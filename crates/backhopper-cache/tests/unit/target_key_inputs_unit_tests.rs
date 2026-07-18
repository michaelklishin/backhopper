// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cache::{TargetKeyInputs, content_hash};
use backhopper_core::model::names::CommitSha;

fn inputs(walk_limit: usize, already_present_enabled: bool) -> TargetKeyInputs {
    TargetKeyInputs {
        resolved_commit: CommitSha::new("a".repeat(40)).unwrap(),
        translations_blake3: "t".to_owned(),
        walk_limit,
        already_present_enabled,
    }
}

// a flag that changes the cached diagnostic must split the key: shared entries would serve a stale tally
#[test]
fn different_walk_limits_hash_to_different_keys() {
    let a = content_hash(&inputs(5000, true)).unwrap();
    let b = content_hash(&inputs(100, true)).unwrap();
    assert_ne!(a, b);
}

#[test]
fn toggling_already_present_changes_the_key() {
    let a = content_hash(&inputs(5000, true)).unwrap();
    let b = content_hash(&inputs(5000, false)).unwrap();
    assert_ne!(a, b);
}

#[test]
fn identical_inputs_hash_identically() {
    let a = content_hash(&inputs(5000, true)).unwrap();
    let b = content_hash(&inputs(5000, true)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_moved_target_tip_changes_the_key() {
    let a = content_hash(&inputs(5000, true)).unwrap();
    let mut moved = inputs(5000, true);
    moved.resolved_commit = CommitSha::new("b".repeat(40)).unwrap();
    let b = content_hash(&moved).unwrap();
    assert_ne!(a, b);
}
