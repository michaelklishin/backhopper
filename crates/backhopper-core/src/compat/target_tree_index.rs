// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Built-once-per-invocation index of every blob path in a target
//! tree. Backs the `--target-repo-dir-path` cross-branch backport
//! analyser. Pure data: construction from a live repository lives in
//! `backhopper-git`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
    /// Assemble an index from already-listed blob paths. The ancestor
    /// directories are derived here so the path and dir views can
    /// never disagree.
    #[must_use]
    pub fn from_parts(
        target_repo: PathBuf,
        target_ref: GitRef,
        resolved_commit: CommitSha,
        present_paths: BTreeSet<PathBuf>,
    ) -> Self {
        let mut present_dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for path in &present_paths {
            for ancestor in path.ancestors().skip(1) {
                if ancestor.as_os_str().is_empty() {
                    continue;
                }
                present_dirs.insert(ancestor.to_path_buf());
            }
        }
        Self {
            target_repo,
            target_ref,
            resolved_commit,
            present_paths,
            present_dirs,
        }
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
