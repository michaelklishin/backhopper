// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves user-supplied `CommitShaPrefix` inputs to full `CommitSha`
//! values: the only path through which raw input becomes a typed SHA.

use std::path::Path;

use backhopper_core::model::names::{CommitSha, CommitShaPrefix};

use backhopper_git::{GitError, GitRepo, ResolvedSha};

use crate::errors::{CliError, CliResult};

pub fn expand_prefix(repo_dir_path: &Path, prefix: &CommitShaPrefix) -> CliResult<CommitSha> {
    if let Some(full) = prefix.as_full_sha() {
        return Ok(full);
    }
    let g = GitRepo::open(repo_dir_path.to_path_buf())?;
    expand_prefix_with(&g, prefix).map_err(|e| enrich_with_repo_path(e, repo_dir_path))
}

pub fn expand_prefix_with(repo: &GitRepo, prefix: &CommitShaPrefix) -> CliResult<CommitSha> {
    if let Some(full) = prefix.as_full_sha() {
        return Ok(full);
    }
    let resolved = repo.resolve_sha_prefix(prefix)?;
    Ok(resolved.commit)
}

/// Expand a prefix against an already-open repo. If the commit is not
/// found, the error names `repo_path` and suggests running `git fetch`.
pub fn expand_prefix_enriched(
    repo: &GitRepo,
    prefix: &CommitShaPrefix,
    repo_path: &Path,
) -> CliResult<CommitSha> {
    expand_prefix_with(repo, prefix).map_err(|e| enrich_with_repo_path(e, repo_path))
}

pub fn resolve_with_kind(repo_dir_path: &Path, prefix: &CommitShaPrefix) -> CliResult<ResolvedSha> {
    let g = GitRepo::open(repo_dir_path.to_path_buf())?;
    Ok(g.resolve_sha_prefix(prefix)?)
}

fn enrich_with_repo_path(err: CliError, repo: &Path) -> CliError {
    match err {
        CliError::Git(GitError::CommitNotFound(prefix)) => CliError::InvalidInput(format!(
            "commit {prefix} not found in repository {}: did you forget to `git fetch`?",
            repo.display()
        )),
        other => other,
    }
}
