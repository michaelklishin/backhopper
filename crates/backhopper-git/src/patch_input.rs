// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared patch resolution for commit-shaped check inputs.
//!
//! Every verb that evaluates "the diff of a commit" (single, merge,
//! multi-series, batch, bisect) resolves through here, so the
//! merge-routing decision is a typed policy instead of duplicated
//! parent-count checks at each call site.

use std::num::NonZeroU32;

use thiserror::Error;

use backhopper_core::compat::source_macros::FileMap;
use backhopper_core::model::names::CommitSha;
use backhopper_core::model::pr_commit::PrCommit;

use crate::errors::GitError;
use crate::pr_commits::pr_commits_for;
use crate::repo::GitRepo;

/// How to treat the commit's parent count at resolution time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePolicy {
    /// Single-commit verbs: a merge SHA is operator error.
    Refuse,
    /// `check merge`: anything that is not a merge is operator error.
    RequireMerge,
    /// Batch-shaped verbs: merges evaluate as the first-parent diff,
    /// the same diff `check merge` produces.
    FirstParentDiff,
}

/// Whether the resolver pays for the inner PR-branch walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrCommitPolicy {
    Collect,
    Skip,
}

/// Where a resolved patch came from. `parent_count` is `NonZeroU32`:
/// root commits are rejected before construction, so 0 is
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitDiffSource {
    Plain,
    Merge { parent_count: NonZeroU32 },
}

impl CommitDiffSource {
    #[must_use]
    pub fn parent_count(self) -> NonZeroU32 {
        match self {
            Self::Plain => NonZeroU32::MIN,
            Self::Merge { parent_count } => parent_count,
        }
    }

    #[must_use]
    pub fn is_merge(self) -> bool {
        matches!(self, Self::Merge { .. })
    }
}

#[derive(Debug, Error)]
pub enum PatchInputError {
    #[error("commit {0} has no parent")]
    RootCommit(CommitSha),

    #[error(
        "{sha} is a merge commit ({parent_count} parents); use 'backhopper check merge {sha}' instead"
    )]
    MergeRefused { sha: CommitSha, parent_count: usize },

    #[error("{sha} is not a merge commit (parents: {parent_count})")]
    NotAMergeCommit { sha: CommitSha, parent_count: usize },

    #[error(transparent)]
    Git(#[from] GitError),
}

/// The patch bytes and provenance of one commit-shaped check input.
#[derive(Debug)]
pub struct ResolvedPatchInput {
    pub sha: CommitSha,
    /// The commit the diff was computed against: the sole parent for
    /// plain commits, the first parent for merges.
    pub diff_base: CommitSha,
    pub bytes: Vec<u8>,
    pub source: CommitDiffSource,
    /// `Some` only for 2-parent merges under `PrCommitPolicy::Collect`.
    pub pr_commits: Option<Vec<PrCommit>>,
}

impl ResolvedPatchInput {
    /// Probe the commit's parents and resolve. One `parents()` call;
    /// everything else follows from it.
    pub fn for_commit(
        repo: &GitRepo,
        sha: &CommitSha,
        merges: MergePolicy,
        pr: PrCommitPolicy,
    ) -> Result<Self, PatchInputError> {
        let parents = repo.parents(sha)?;
        Self::from_parents(repo, sha, &parents, merges, pr)
    }

    /// Resolve with an already-probed parent list. Batch planning
    /// probes every line up front; this variant avoids a second
    /// `parents()` lookup per commit.
    pub fn from_parents(
        repo: &GitRepo,
        sha: &CommitSha,
        parents: &[CommitSha],
        merges: MergePolicy,
        pr: PrCommitPolicy,
    ) -> Result<Self, PatchInputError> {
        match parents {
            [] => Err(PatchInputError::RootCommit(sha.clone())),
            [parent] => {
                if merges == MergePolicy::RequireMerge {
                    return Err(PatchInputError::NotAMergeCommit {
                        sha: sha.clone(),
                        parent_count: 1,
                    });
                }
                Ok(Self {
                    sha: sha.clone(),
                    diff_base: parent.clone(),
                    bytes: diff_bytes(repo, parent, sha)?,
                    source: CommitDiffSource::Plain,
                    pr_commits: None,
                })
            }
            [first, ..] => {
                if merges == MergePolicy::Refuse {
                    return Err(PatchInputError::MergeRefused {
                        sha: sha.clone(),
                        parent_count: parents.len(),
                    });
                }
                let parent_count = u32::try_from(parents.len())
                    .ok()
                    .and_then(NonZeroU32::new)
                    .expect("a non-empty parent list has a non-zero count");
                let pr_commits = match pr {
                    PrCommitPolicy::Collect => pr_commits_for(repo, sha)?,
                    PrCommitPolicy::Skip => None,
                };
                Ok(Self {
                    sha: sha.clone(),
                    diff_base: first.clone(),
                    bytes: diff_bytes(repo, first, sha)?,
                    source: CommitDiffSource::Merge { parent_count },
                    pr_commits,
                })
            }
        }
    }

    /// Read every `.erl` and `.hrl` blob at the diff base for macro
    /// tables. Separate from resolution: `bisect` parses the patch
    /// without macro context and skips this cost entirely. An empty
    /// patch touches nothing, so it skips the tree read too.
    pub fn load_source_files(&self, repo: &GitRepo) -> Result<FileMap, GitError> {
        if self.bytes.is_empty() {
            return Ok(FileMap::new());
        }
        load_files_at(repo, &self.diff_base)
    }
}

/// The root dep-pin manifest, the only pin file backhopper reads:
/// the monorepo carries copies under `deps/*/`, which are vendored
/// noise.
pub const COMPONENTS_MK_PATH: &str = "rabbitmq-components.mk";

/// Paths worth carrying in analysis diffs: Erlang and Elixir sources,
/// cuttlefish `.schema`, `.snippets`, and `.partial` files, plus the root
/// dep-pin manifest, so commit-shaped inputs classify and detect pin
/// bumps the same way patch-file inputs do.
pub fn analyzable_diff_path(p: &str) -> bool {
    p.ends_with(".erl")
        || p.ends_with(".hrl")
        || p.ends_with(".ex")
        || p.ends_with(".exs")
        || p.ends_with(".schema")
        || p.ends_with(".snippets")
        || p.ends_with(".partial")
        || p == COMPONENTS_MK_PATH
}

pub(crate) fn diff_bytes(
    repo: &GitRepo,
    from: &CommitSha,
    to: &CommitSha,
) -> Result<Vec<u8>, GitError> {
    let text = repo.diff_commits_unified(from, to, analyzable_diff_path)?;
    Ok(text.into_bytes())
}

/// Read every `.erl` and `.hrl` blob at `commit` into a `FileMap`, plus
/// the cuttlefish `.schema`, `.snippets`, and `.partial` files whose full
/// content the reference extractor needs. The evaluation builds macro
/// tables from the Erlang entries; callers that can prove a patch is
/// empty should skip this entirely.
pub fn load_files_at(repo: &GitRepo, commit: &CommitSha) -> Result<FileMap, GitError> {
    let blobs = repo.read_paths_at_commit(commit, source_content_path)?;
    let mut map = FileMap::new();
    for blob in blobs {
        if let Ok(text) = String::from_utf8(blob.bytes) {
            map.insert(blob.path, text);
        }
    }
    Ok(map)
}

/// Paths whose full content the evaluator loads: Erlang sources and
/// headers for macro context, and cuttlefish files for schema reference
/// extraction.
pub fn source_content_path(p: &str) -> bool {
    p.ends_with(".erl")
        || p.ends_with(".hrl")
        || p.ends_with(".schema")
        || p.ends_with(".snippets")
        || p.ends_with(".partial")
}
