// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Tier-1 already-present detection: did this candidate's patch
//! already land on the target branch under another SHA?
//!
//! Three mechanisms, cheapest first: trailer-origin ancestry, trailer
//! intersection over a bounded first-parent walk, and normalized
//! patch-hash equality over the same walk. All exact; the heuristic
//! content tier lives in `backhopper-core`.

use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::model::names::CommitSha;
use backhopper_core::model::verdict::{TargetMatch, TargetMatchKind};

use crate::errors::GitError;
use crate::repo::GitRepo;
use crate::walk::{WalkedCommit, cherry_pick_trailers, first_parent_walk_since, patch_id};

/// Everything that identifies a backport candidate to the target-side
/// scan.
#[derive(Debug, Clone)]
pub struct CandidateIdentity {
    pub sha: CommitSha,
    /// `-x` trailer origins from the candidate's own message.
    pub trailer_origins: Vec<CommitSha>,
    /// Inner commit SHAs for 2-parent merge candidates.
    pub inner_pr_shas: Vec<CommitSha>,
    /// Normalized hash of the candidate's resolved patch, `None` for
    /// an empty diff.
    pub patch_hash: Option<String>,
    pub touched_paths: Vec<PathBuf>,
}

impl CandidateIdentity {
    /// The full identity set a target trailer can name.
    fn identity_set(&self) -> BTreeSet<&CommitSha> {
        let mut set: BTreeSet<&CommitSha> = BTreeSet::new();
        set.insert(&self.sha);
        set.extend(self.trailer_origins.iter());
        set.extend(self.inner_pr_shas.iter());
        set
    }
}

/// One walked target commit with its lazily computed identities. The
/// `OnceCell`s make the patch-hash pass pay only for commits whose
/// touched paths intersect the candidate's.
#[derive(Debug)]
struct IndexRow {
    commit: WalkedCommit,
    trailers: Vec<CommitSha>,
    changed_paths: OnceCell<Option<BTreeSet<PathBuf>>>,
    patch: OnceCell<Option<String>>,
}

impl IndexRow {
    fn changed_paths(&self, repo: &GitRepo) -> Option<&BTreeSet<PathBuf>> {
        self.changed_paths
            .get_or_init(|| {
                let base = self.commit.parents.first()?;
                match repo.changed_paths(base, &self.commit.sha) {
                    Ok(paths) => Some(paths),
                    Err(e) => {
                        tracing::warn!(
                            sha = %self.commit.sha,
                            error = %e,
                            "changed-paths read failed; commit excluded from patch-id matching"
                        );
                        None
                    }
                }
            })
            .as_ref()
    }

    fn patch_hash(&self, repo: &GitRepo) -> Option<&str> {
        self.patch
            .get_or_init(|| {
                let base = self.commit.parents.first()?;
                match patch_id(repo, base, &self.commit.sha) {
                    Ok(id) => id.map(|p| p.as_str().to_owned()),
                    Err(e) => {
                        tracing::warn!(
                            sha = %self.commit.sha,
                            error = %e,
                            "patch-id computation failed; commit excluded from matching"
                        );
                        None
                    }
                }
            })
            .as_deref()
    }
}

/// A bounded first-parent window of the target branch, built once per
/// `(tip, since)` pair and shared across a batch's candidates.
#[derive(Debug)]
pub struct TargetWalkIndex {
    rows: Vec<IndexRow>,
    /// True when `walk_limit` cut the window short of `since`.
    pub truncated: bool,
}

impl TargetWalkIndex {
    pub fn build(
        repo: &GitRepo,
        tip: &CommitSha,
        since: &CommitSha,
        walk_limit: usize,
    ) -> Result<Self, GitError> {
        let mut walked = first_parent_walk_since(repo, tip, since)?;
        let truncated = walked.len() > walk_limit;
        walked.truncate(walk_limit);
        let rows = walked
            .into_iter()
            .map(|commit| IndexRow {
                trailers: cherry_pick_trailers(&commit.message),
                commit,
                changed_paths: OnceCell::new(),
                patch: OnceCell::new(),
            })
            .collect();
        Ok(Self { rows, truncated })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Scan the window for the candidate. Trailer intersection first
    /// (no git reads), then patch-hash equality restricted to commits
    /// whose changed paths intersect the candidate's.
    pub fn find_match(&self, repo: &GitRepo, candidate: &CandidateIdentity) -> Option<TargetMatch> {
        let identity = candidate.identity_set();
        for row in &self.rows {
            if row.trailers.iter().any(|t| identity.contains(t)) {
                return Some(TargetMatch {
                    commit: row.commit.sha.clone(),
                    via: TargetMatchKind::TrailerIntersection,
                    subject: row.commit.subject.clone(),
                });
            }
        }
        let candidate_hash = candidate.patch_hash.as_deref()?;
        let touched: BTreeSet<&PathBuf> = candidate.touched_paths.iter().collect();
        if touched.is_empty() {
            return None;
        }
        for row in &self.rows {
            let Some(paths) = row.changed_paths(repo) else {
                continue;
            };
            if !paths.iter().any(|p| touched.contains(p)) {
                continue;
            }
            if row.patch_hash(repo) == Some(candidate_hash) {
                return Some(TargetMatch {
                    commit: row.commit.sha.clone(),
                    via: TargetMatchKind::PatchId,
                    subject: row.commit.subject.clone(),
                });
            }
        }
        None
    }
}

/// Tier 1b: is any of the candidate's own trailer origins already an
/// ancestor of the target tip? Catches origins that landed before the
/// walk window (a `main` commit the target branched after). Origins
/// absent from the target object store are skipped, not errors.
pub fn trailer_origin_on_target(
    repo: &GitRepo,
    origins: &[CommitSha],
    tip: &CommitSha,
) -> Result<Option<TargetMatch>, GitError> {
    for origin in origins {
        if !repo.has_commit(origin)? {
            continue;
        }
        if repo.is_ancestor(origin, tip)? {
            let subject = repo.commit_subject(origin)?;
            return Ok(Some(TargetMatch {
                commit: origin.clone(),
                via: TargetMatchKind::TrailerAncestry,
                subject,
            }));
        }
    }
    Ok(None)
}
