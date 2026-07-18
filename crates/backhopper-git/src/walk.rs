// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! First-parent window walks and cascade-suppression primitives for
//! `siblings doctor`.
//!
//! The walk reads each commit once and carries everything later
//! pipeline stages need (parents, committer time, subject, full
//! message), so the vocabulary prefilter and the trailer scan never
//! re-open commit objects.

use std::sync::LazyLock;
use std::time::Duration;

use gix::bstr::ByteSlice;
use regex::Regex;
use time::OffsetDateTime;

use backhopper_core::model::names::CommitSha;

use crate::errors::{GitError, gix_err};
use crate::patch_input::diff_bytes;
use crate::pr_commits::commit_subject;
use crate::repo::GitRepo;

/// Tolerated committer-clock skew when the since-point is not on the
/// walked first-parent chain and the window has to be bounded by
/// time instead of ancestry.
const WALK_TIME_SLACK: Duration = Duration::from_hours(24);

/// One commit captured by a first-parent window walk.
#[derive(Debug, Clone)]
pub struct WalkedCommit {
    pub sha: CommitSha,
    pub parents: Vec<CommitSha>,
    pub committed_at: OffsetDateTime,
    pub subject: String,
    pub message: String,
}

/// Walk first-parent links from `tip` back to `since`, newest first,
/// excluding `since` itself.
///
/// When `since` sits on the chain (the same-branch case, e.g. a
/// release tag of the walked branch), the window is pure ancestry.
/// When it does not (the cross-branch case: bounding a `main` walk by
/// a release tag that is on a stable branch), the window keeps
/// commits whose committer time is strictly newer than `since`'s,
/// and the walk stops once times fall a day older than that bound.
pub fn first_parent_walk_since(
    repo: &GitRepo,
    tip: &CommitSha,
    since: &CommitSha,
) -> Result<Vec<WalkedCommit>, GitError> {
    let since_time = repo.commit_timestamp(since)?;
    let cutoff = since_time - WALK_TIME_SLACK;
    // Same-branch windows are pure ancestry: a commit dated before since (a backport
    // or rebase) stays in; the time cutoff only bounds the cross-branch walk.
    let same_branch = repo.is_ancestor(since, tip)?;
    let mut out: Vec<WalkedCommit> = Vec::new();
    let mut found_since = false;
    let mut current = Some(tip.clone());
    while let Some(sha) = current {
        if sha == *since {
            found_since = true;
            break;
        }
        let info = read_walked(repo, &sha)?;
        if !same_branch && info.committed_at < cutoff {
            break;
        }
        current = info.parents.first().cloned();
        out.push(info);
    }
    // since is not on the first-parent chain (cross-branch, or reachable only
    // via a merge's second parent): fall back to the committer-time window.
    if !found_since {
        out.retain(|c| c.committed_at > since_time);
    }
    Ok(out)
}

fn read_walked(repo: &GitRepo, sha: &CommitSha) -> Result<WalkedCommit, GitError> {
    let gix_repo = repo.gix();
    let oid = gix::ObjectId::from_hex(sha.as_str().as_bytes()).map_err(gix_err)?;
    let commit = gix_repo
        .find_object(oid)
        .map_err(|_| GitError::CommitNotFound(sha.to_string()))?
        .try_into_commit()
        .map_err(gix_err)?;
    let mut parents = Vec::new();
    for id in commit.parent_ids() {
        parents.push(CommitSha::new(id.to_string()).map_err(gix_err)?);
    }
    let time = commit.time().map_err(gix_err)?;
    let committed_at = OffsetDateTime::from_unix_timestamp(time.seconds)
        .map_err(|e| GitError::Gix(format!("committer time out of range: {e}")))?;
    let subject = commit_subject(&commit)?;
    let message = commit
        .message_raw()
        .map_err(gix_err)?
        .to_str_lossy()
        .into_owned();
    Ok(WalkedCommit {
        sha: sha.clone(),
        parents,
        committed_at,
        subject,
        message,
    })
}

static CHERRY_PICK_TRAILER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\(cherry picked from commit ([0-9a-fA-F]{40})\)")
        .expect("cherry-pick trailer regex must compile")
});

/// Every source SHA named by a `git cherry-pick -x` trailer in
/// `message`, in order of appearance. A pick of a pick carries one
/// trailer per hop; all of them count.
#[must_use]
pub fn cherry_pick_trailers(message: &str) -> Vec<CommitSha> {
    CHERRY_PICK_TRAILER
        .captures_iter(message)
        .filter_map(|c| CommitSha::new(c[1].to_lowercase()).ok())
        .collect()
}

/// Content identity of one commit's first-parent diff, used to match
/// hand-landed cherry-picks that carry no `-x` trailer. Not git's
/// `patch-id`: both sides of a comparison are always produced by this
/// same function, so cross-tool compatibility is not needed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PatchId(String);

impl PatchId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Compute the `PatchId` of `sha`'s diff against `base` (its first
/// parent). Hunk header lines are excluded so a pick applied at a
/// different line offset keeps the same identity. Returns `None` for
/// an empty diff: empty diffs would all share one id and suppress
/// each other.
pub fn patch_id(
    repo: &GitRepo,
    base: &CommitSha,
    sha: &CommitSha,
) -> Result<Option<PatchId>, GitError> {
    let bytes = diff_bytes(repo, base, sha)?;
    Ok(normalized_patch_hash(&bytes).map(PatchId))
}

/// BLAKE3 hex over a unified diff with the `@@` hunk-header lines
/// excluded, so pure offset drift keeps the identity while any
/// content-line or path drift changes it. Returns `None` for an
/// empty diff. Shared by the sibling-drift `PatchId` and the verdict
/// cache's content key.
#[must_use]
pub fn normalized_patch_hash(diff: &[u8]) -> Option<String> {
    // hash raw bytes: a lossy UTF-8 decode would collide distinct invalid byte runs
    if diff.is_empty() {
        return None;
    }
    // match str::lines: a single trailing newline yields no empty tail line
    let diff = diff.strip_suffix(b"\n").unwrap_or(diff);
    let mut hasher = blake3::Hasher::new();
    let mut hashed_any = false;
    for line in diff.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"@@") {
            continue;
        }
        hasher.update(line);
        hasher.update(b"\n");
        hashed_any = true;
    }
    if !hashed_any {
        return None;
    }
    Some(hasher.finalize().to_hex().to_string())
}
