// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! gix-backed git access for `backhopper`: the workspace's only git
//! seam. `backhopper-core` stays gix-free; everything that opens a
//! repository lives here.

pub mod errors;
pub mod patch_input;
pub mod pr_commits;
pub mod repo;
pub mod target_tree;
pub mod walk;

pub use errors::GitError;
pub use patch_input::{
    CommitDiffSource, MergePolicy, PatchInputError, PrCommitPolicy, ResolvedPatchInput,
    load_files_at,
};
pub use pr_commits::{classify, pr_commits_for};
pub use repo::{
    AMBIGUOUS_SHA_CANDIDATE_CAP, BlobAtPath, GitRepo, ObjectKind, ResolvedSha, TagListing,
    unified_diff_body,
};
pub use target_tree::build_target_tree_index;
pub use walk::{
    PatchId, WalkedCommit, cherry_pick_trailers, first_parent_walk_since, normalized_patch_hash,
    patch_id,
};
