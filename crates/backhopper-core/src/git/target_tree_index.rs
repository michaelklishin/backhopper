// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Built-once-per-invocation index of every blob path in a target
//! tree. Backs the `--target-repo-dir-path` cross-branch backport
//! analyser.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::errors::GitError;
use crate::git::GitRepo;
use crate::model::names::{CommitSha, GitRef};

#[derive(Debug, Clone)]
pub struct TargetTreeIndex {
    target_repo: PathBuf,
    target_ref: GitRef,
    resolved_commit: CommitSha,
    present_paths: BTreeSet<PathBuf>,
    present_dirs: BTreeSet<PathBuf>,
}

impl TargetTreeIndex {
    pub fn build(repo: &GitRepo, target_ref: &GitRef) -> Result<Self, GitError> {
        let resolved_commit = repo.resolve_rev(target_ref.as_str())?;
        let blobs = repo.read_paths_at_commit(&resolved_commit, |_| true)?;
        let mut present_paths: BTreeSet<PathBuf> = BTreeSet::new();
        let mut present_dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for blob in &blobs {
            present_paths.insert(blob.path.clone());
            for ancestor in blob.path.ancestors().skip(1) {
                if ancestor.as_os_str().is_empty() {
                    continue;
                }
                present_dirs.insert(ancestor.to_path_buf());
            }
        }
        if present_paths.is_empty() {
            return Err(GitError::Gix(format!(
                "target repo {} resolves to a commit with no blobs at {}; refusing to build empty index",
                repo.path().display(),
                target_ref
            )));
        }
        Ok(Self {
            target_repo: repo.path().to_path_buf(),
            target_ref: target_ref.clone(),
            resolved_commit,
            present_paths,
            present_dirs,
        })
    }

    pub fn target_repo(&self) -> &Path {
        &self.target_repo
    }

    pub fn target_ref(&self) -> &GitRef {
        &self.target_ref
    }

    pub fn resolved_commit(&self) -> &CommitSha {
        &self.resolved_commit
    }

    pub fn contains_path(&self, p: &Path) -> bool {
        self.present_paths.contains(p)
    }

    pub fn contains_dir(&self, p: &Path) -> bool {
        self.present_dirs.contains(p)
    }

    pub fn path_count(&self) -> usize {
        self.present_paths.len()
    }

    pub fn dir_count(&self) -> usize {
        self.present_dirs.len()
    }

    pub fn present_paths(&self) -> &BTreeSet<PathBuf> {
        &self.present_paths
    }
}
