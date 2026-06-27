// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Built-once-per-invocation index of every blob path in a target
//! tree. Backs the `--target-repo-dir-path` cross-branch backport
//! analyser. Pure data: construction from a live repository lives in
//! `backhopper-git`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::model::names::{CommitSha, GitRef, ModuleName};

#[derive(Debug, Clone)]
pub struct TargetTreeIndex {
    target_repo: PathBuf,
    target_ref: GitRef,
    resolved_commit: CommitSha,
    present_paths: BTreeSet<PathBuf>,
    present_dirs: BTreeSet<PathBuf>,
    erl_modules: BTreeMap<ModuleName, PathBuf>,
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
        // Erlang fixes module name to file basename. A basename seen
        // twice is a clash Erlang itself rejects, so record neither.
        let mut by_module: BTreeMap<ModuleName, Option<PathBuf>> = BTreeMap::new();
        for path in &present_paths {
            for ancestor in path.ancestors().skip(1) {
                if ancestor.as_os_str().is_empty() {
                    continue;
                }
                present_dirs.insert(ancestor.to_path_buf());
            }
            if path.extension().and_then(|e| e.to_str()) == Some("erl")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(module) = ModuleName::from_str(stem)
            {
                by_module
                    .entry(module)
                    .and_modify(|slot| *slot = None)
                    .or_insert_with(|| Some(path.clone()));
            }
        }
        let erl_modules = by_module
            .into_iter()
            .filter_map(|(m, p)| p.map(|p| (m, p)))
            .collect();
        Self {
            target_repo,
            target_ref,
            resolved_commit,
            present_paths,
            present_dirs,
            erl_modules,
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

    /// The `.erl` file that defines `module`, by basename, or `None`
    /// when the tree has no such file or two files share the basename.
    pub fn module_erl_path(&self, module: &ModuleName) -> Option<&Path> {
        self.erl_modules.get(module).map(PathBuf::as_path)
    }
}
