// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Inner PR-branch commit enumeration under a 2-parent merge.
//!
//! Given a merge commit `M` with parents `M^1` (base-branch tip) and
//! `M^2` (PR-branch tip), this module walks every commit reachable
//! from `M^2` but not `M^1`, classifying each via the conjunction of a
//! subject regex catalog and a strict file-set subset rule against
//! prior PR-branch commits.

use std::collections::BTreeSet;
use std::fmt::Display;
use std::ops::ControlFlow;
use std::sync::LazyLock;

use gix::bstr::ByteSlice;
use regex::RegexSet;
use time::OffsetDateTime;

use crate::errors::GitError;
use crate::git::GitRepo;
use crate::model::names::{CommitSha, RelativePath};
use crate::model::pr_commit::{PrCommit, PrCommitKind};

fn gix_err<E: Display>(e: E) -> GitError {
    GitError::Gix(e.to_string())
}

/// Enumerate the PR-branch commits inside a 2-parent merge.
///
/// Returns `Ok(None)` for non-2-parent commits (octopus merges and
/// non-merges). Returns `Ok(Some(vec![]))` for a 2-parent merge whose
/// PR-branch walk is empty (rare but real; e.g. a fast-forward that
/// still resulted in a 2-parent merge object).
///
/// Walk order is topological, oldest first.
pub fn pr_commits_for(
    repo: &GitRepo,
    merge: &CommitSha,
) -> Result<Option<Vec<PrCommit>>, GitError> {
    let gix_repo = repo.gix();
    let merge_id = gix::ObjectId::from_hex(merge.as_str().as_bytes()).map_err(gix_err)?;
    let merge_commit = gix_repo
        .find_object(merge_id)
        .map_err(|_| GitError::CommitNotFound(merge.to_string()))?
        .try_into_commit()
        .map_err(gix_err)?;
    let parents: Vec<gix::ObjectId> = merge_commit.parent_ids().map(Into::into).collect();
    if parents.len() != 2 {
        return Ok(None);
    }
    let base_parent = parents[0];
    let pr_parent = parents[1];

    let walk_ids = walk_pr_branch(gix_repo, base_parent, pr_parent)?;

    let mut prior_union: BTreeSet<RelativePath> = BTreeSet::new();
    let mut out: Vec<PrCommit> = Vec::with_capacity(walk_ids.len());
    for id in walk_ids {
        let commit = gix_repo
            .find_object(id)
            .map_err(gix_err)?
            .try_into_commit()
            .map_err(gix_err)?;
        let subject = commit_subject(&commit)?;
        let author_date = commit_author_date(&commit)?;
        let files = files_touched_relative_to_first_parent(gix_repo, &commit)?;

        let regex_hit = SUBJECT_CATALOG.matches(&subject);
        let strict_subset = !files.is_empty()
            && !prior_union.is_empty()
            && files.iter().all(|p| prior_union.contains(p))
            && files.len() < prior_union.len();

        let kind = if regex_hit.matched_any() && strict_subset {
            let first_idx = regex_hit.iter().next().expect("at least one match");
            CATALOG_KINDS[first_idx]
        } else {
            PrCommitKind::Substantive
        };

        out.push(PrCommit {
            sha: CommitSha::new(id.to_string()).map_err(gix_err)?,
            subject,
            author_date,
            kind,
        });
        prior_union.extend(files);
    }
    Ok(Some(out))
}

/// Topologically-sorted commits reachable from `pr_parent` but not
/// from `base_parent`, oldest first.
fn walk_pr_branch(
    repo: &gix::Repository,
    base_parent: gix::ObjectId,
    pr_parent: gix::ObjectId,
) -> Result<Vec<gix::ObjectId>, GitError> {
    let mut commits: Vec<gix::ObjectId> = repo
        .rev_walk([pr_parent])
        .with_hidden([base_parent])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(gix_err)?
        .map(|res| res.map(|info| info.id))
        .collect::<Result<_, _>>()
        .map_err(gix_err)?;
    commits.reverse();
    Ok(commits)
}

pub(super) fn commit_subject(commit: &gix::Commit<'_>) -> Result<String, GitError> {
    let message = commit.message().map_err(gix_err)?;
    let raw = message.title.to_str_lossy();
    Ok(raw.trim_end_matches(['\n', '\r']).to_owned())
}

fn commit_author_date(commit: &gix::Commit<'_>) -> Result<OffsetDateTime, GitError> {
    let actor = commit.author().map_err(gix_err)?;
    let parsed = gix::date::parse(actor.time, None)
        .map_err(|e| GitError::Gix(format!("author time unparseable: {e}")))?;
    OffsetDateTime::from_unix_timestamp(parsed.seconds)
        .map_err(|e| GitError::Gix(format!("author time out of range: {e}")))
}

/// Returns the set of paths touched by `commit` relative to its first
/// parent. Used by the strict-subset rule.
fn files_touched_relative_to_first_parent(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
) -> Result<BTreeSet<RelativePath>, GitError> {
    let new_tree = commit.tree().map_err(gix_err)?;
    let old_tree = match commit.parent_ids().next() {
        Some(parent_id) => repo
            .find_object(parent_id)
            .map_err(gix_err)?
            .try_into_commit()
            .map_err(gix_err)?
            .tree()
            .map_err(gix_err)?,
        None => return Ok(BTreeSet::new()),
    };
    let mut out: BTreeSet<RelativePath> = BTreeSet::new();
    let mut platform = old_tree.changes().map_err(gix_err)?;
    platform
        .options(|opts| {
            opts.track_rewrites(None);
        })
        .for_each_to_obtain_tree(&new_tree, |change| -> Result<_, GitError> {
            if let Ok(rel) = RelativePath::try_from(change.location()) {
                out.insert(rel);
            }
            Ok(ControlFlow::Continue(()))
        })
        .map_err(gix_err)?;
    Ok(out)
}

/// Catalog of subjects that suggest a bookkeeping role plus the
/// `PrCommitKind` each entry maps to.
const CATALOG: &[(&str, PrCommitKind)] = &[
    // git-generated merge subjects — conflict resolution by convention
    (
        r"(?i)^Merge branch '[^']+' (into|of) \S+$",
        PrCommitKind::ConflictResolution,
    ),
    (
        r"(?i)^Merge branch '[^']+'$",
        PrCommitKind::ConflictResolution,
    ),
    (
        r"(?i)^Merge remote-tracking branch '[^']+' into \S+$",
        PrCommitKind::ConflictResolution,
    ),
    (
        r"(?i)^Merge remote-tracking branch '[^']+'$",
        PrCommitKind::ConflictResolution,
    ),
    // bare conflict subjects
    (r"(?i)^Conflicts?\.?$", PrCommitKind::ConflictResolution),
    // verb + conflict noun
    (
        r"(?i)^(Resolve[ds]?|Fix(es|ed)?|Address(es|ed)?|Handle[ds]?) (merge )?conflicts?\.?$",
        PrCommitKind::ConflictResolution,
    ),
    // descriptive "merge conflict" phrase mid-subject
    (
        r"(?i)\bmerge conflict resolution\b",
        PrCommitKind::ConflictResolution,
    ),
    // GitHub UI canonical apply-suggestion commit
    (
        r"(?i)^Apply suggestions from code review$",
        PrCommitKind::ReviewFeedback,
    ),
    // review feedback patterns (guarded by the subset rule)
    (
        r"(?i)^(Address(es|ed)?|Apply|Applies|Applied|Handle[ds]?)\s+(PR\s+)?((review|code\s+review|requested)\s+)?(feedback|comments?|suggestions?|changes?)(\s+#\d+)?\.?$",
        PrCommitKind::ReviewFeedback,
    ),
    // git --fixup auto-generated
    (r"(?i)^fixup!\s+\S", PrCommitKind::Fixup),
    // manual fixup convention
    (r"(?i)^fixup:\s+\S", PrCommitKind::Fixup),
    // work in progress
    (r"(?i)^WIP(:\s+.+)?$", PrCommitKind::WipOrCleanup),
    // bare cleanup
    (r"(?i)^[Cc]leanup\.?$", PrCommitKind::WipOrCleanup),
];

const CATALOG_KINDS: &[PrCommitKind] = &kinds_from_catalog();

const fn kinds_from_catalog() -> [PrCommitKind; CATALOG.len()] {
    let mut out = [PrCommitKind::Substantive; CATALOG.len()];
    let mut i = 0;
    while i < CATALOG.len() {
        out[i] = CATALOG[i].1;
        i += 1;
    }
    out
}

static SUBJECT_CATALOG: LazyLock<RegexSet> = LazyLock::new(|| {
    let patterns: Vec<&str> = CATALOG.iter().map(|e| e.0).collect();
    RegexSet::new(patterns).expect("pr-commit catalog regex set must compile")
});

/// Classify a subject + subset-rule outcome into a `PrCommitKind`.
///
/// Exposed for unit testing of the catalog without needing a real repo.
#[must_use]
pub fn classify(subject: &str, strict_subset: bool) -> PrCommitKind {
    if !strict_subset {
        return PrCommitKind::Substantive;
    }
    let m = SUBJECT_CATALOG.matches(subject);
    m.iter()
        .next()
        .map_or(PrCommitKind::Substantive, |idx| CATALOG_KINDS[idx])
}
