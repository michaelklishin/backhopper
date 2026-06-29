// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `build_target_tree_index` reads the named ref's tree, not the working
//! tree, so a consumer can predict against any ref without a checkout.

use std::path::Path;

use backhopper_core::model::names::GitRef;
use backhopper_git::{GitRepo, build_target_tree_index};

use backhopper_test_support::GitRepoFixture;

// A file present at the tagged commit but deleted at HEAD is in the index
// built from the tag and absent from the one built from HEAD: the index
// follows the ref, not the checked-out working tree.
#[test]
fn index_reflects_the_named_ref_not_the_working_tree() {
    let repo = GitRepoFixture::new();
    // a file that survives both commits keeps HEAD's tree non-empty
    repo.write_file("src/keep.erl", "-module(keep).\n");
    repo.write_file("src/osiris_log.erl", "-module(osiris_log).\n");
    repo.commit("add osiris_log");
    repo.tag("rel");
    std::fs::remove_file(repo.dir.path().join("src/osiris_log.erl")).unwrap();
    repo.stage_all();
    repo.commit("remove osiris_log");

    let g = GitRepo::open(repo.dir.path()).unwrap();
    let at_tag = build_target_tree_index(&g, &GitRef::new("rel").unwrap()).unwrap();
    let at_head = build_target_tree_index(&g, &GitRef::new("HEAD").unwrap()).unwrap();

    assert!(at_tag.contains_path(Path::new("src/osiris_log.erl")));
    assert!(!at_head.contains_path(Path::new("src/osiris_log.erl")));
    assert_eq!(at_tag.resolved_commit().as_str(), repo.rev_parse("rel"));
}
