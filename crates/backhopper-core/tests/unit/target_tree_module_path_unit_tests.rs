// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `TargetTreeIndex::module_erl_path`: the module-to-path derivation
//! over `present_paths`, with collision suppression.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{CommitSha, GitRef, ModuleName};

fn index(paths: &[&str]) -> TargetTreeIndex {
    let present: BTreeSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
    TargetTreeIndex::from_parts(
        PathBuf::from("/repo"),
        GitRef::new("HEAD").unwrap(),
        CommitSha::new("a".repeat(40)).unwrap(),
        present,
    )
}

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

#[test]
fn a_module_resolves_to_its_erl_path() {
    let idx = index(&[
        "deps/rabbit/src/rabbit_misc.erl",
        "deps/rabbit/src/other.erl",
    ]);
    assert_eq!(
        idx.module_erl_path(&module("rabbit_misc")),
        Some(Path::new("deps/rabbit/src/rabbit_misc.erl"))
    );
}

#[test]
fn a_module_with_no_erl_file_is_none() {
    let idx = index(&["deps/rabbit/src/rabbit_misc.erl"]);
    assert_eq!(idx.module_erl_path(&module("absent")), None);
}

// A basename seen twice is a clash Erlang itself rejects, so the index
// records neither: a qualified call to it classifies Unknown.
#[test]
fn a_basename_collision_resolves_to_none() {
    let idx = index(&["deps/a/src/dup.erl", "deps/b/src/dup.erl"]);
    assert_eq!(idx.module_erl_path(&module("dup")), None);
}

#[test]
fn a_non_erl_file_does_not_register_a_module() {
    let idx = index(&["deps/rabbit/src/rabbit_misc.hrl"]);
    assert_eq!(idx.module_erl_path(&module("rabbit_misc")), None);
}
