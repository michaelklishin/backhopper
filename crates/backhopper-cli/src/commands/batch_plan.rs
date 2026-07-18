// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Phase one of `check batch`: validate every commits-file line
//! before evaluating any. A `BatchPlan` can only be constructed
//! fully resolved and parent-probed, so phase two never aborts
//! mid-loop on operator input and every bad line surfaces at once.

use std::path::Path;

use backhopper_core::model::names::{CommitSha, CommitShaPrefix};
use backhopper_git::GitRepo;

use crate::commands::sha_prefix::expand_prefix_enriched;
use crate::errors::{CliError, CliResult};

/// One resolved, parent-probed commits-file line.
#[derive(Debug)]
pub struct PlannedCommit {
    pub sha: CommitSha,
    pub parents: Vec<CommitSha>,
}

#[derive(Debug)]
pub struct BatchPlan {
    commits: Vec<PlannedCommit>,
}

impl BatchPlan {
    /// Resolve every `(line number, prefix)` entry and probe its
    /// parents. Failures aggregate into one error in line order:
    /// unresolvable and ambiguous prefixes, non-commit objects, and
    /// root commits all surface together.
    pub fn resolve(
        repo: &GitRepo,
        repo_path: &Path,
        entries: &[(usize, CommitShaPrefix)],
    ) -> CliResult<Self> {
        let mut commits = Vec::with_capacity(entries.len());
        let mut failures: Vec<String> = Vec::new();
        for (line_no, prefix) in entries {
            let sha = match expand_prefix_enriched(repo, prefix, repo_path) {
                Ok(sha) => sha,
                Err(e) => {
                    failures.push(format!("line {line_no}: {e}"));
                    continue;
                }
            };
            match repo.parents(&sha) {
                Ok(parents) if parents.is_empty() => {
                    failures.push(format!("line {line_no}: commit {sha} has no parent"));
                }
                Ok(parents) => commits.push(PlannedCommit { sha, parents }),
                Err(e) => failures.push(format!("line {line_no}: {e}")),
            }
        }
        if !failures.is_empty() {
            let detail = failures.join("\n  ");
            return Err(CliError::InvalidInput(format!(
                "{} commits-file line(s) failed to resolve:\n  {detail}",
                failures.len()
            )));
        }
        Ok(Self { commits })
    }

    pub fn commits(&self) -> &[PlannedCommit] {
        &self.commits
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }
}
