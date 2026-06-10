// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! gix-backed construction of the pure `TargetTreeIndex` in
//! `backhopper-core`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::GitRef;

use crate::errors::GitError;
use crate::repo::GitRepo;

/// Build a `TargetTreeIndex` by reading every blob path at
/// `target_ref` in `repo`. Refuses an empty tree: an index with no
/// paths would classify every touched path as missing.
pub fn build_target_tree_index(
    repo: &GitRepo,
    target_ref: &GitRef,
) -> Result<TargetTreeIndex, GitError> {
    let resolved_commit = repo.resolve_rev(target_ref.as_str())?;
    let blobs = repo.read_paths_at_commit(&resolved_commit, |_| true)?;
    let present_paths: BTreeSet<PathBuf> = blobs.into_iter().map(|b| b.path).collect();
    if present_paths.is_empty() {
        return Err(GitError::Gix(format!(
            "target repo {} resolves to a commit with no blobs at {}; refusing to build empty index",
            repo.path().display(),
            target_ref
        )));
    }
    Ok(TargetTreeIndex::from_parts(
        repo.path().to_path_buf(),
        target_ref.clone(),
        resolved_commit,
        present_paths,
    ))
}
