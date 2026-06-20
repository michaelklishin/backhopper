// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cache::{CacheKeyInputs, EvaluationShape, PinKeyRow, TargetKeyInputs};
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};

fn pin(snapshot: &str) -> PinKeyRow {
    PinKeyRow {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v2.0.0").unwrap(),
        resolved_tag_sha: Some(CommitSha::new("a".repeat(40)).unwrap()),
        snapshot_blake3: snapshot.to_owned(),
    }
}

fn key() -> CacheKeyInputs {
    CacheKeyInputs {
        shape: EvaluationShape::Check,
        commit: CommitSha::new("a".repeat(40)).unwrap(),
        series: Some(SeriesName::new("rabbitmq-4.1").unwrap()),
        pins: vec![pin("snap-a")],
        source_pins: Vec::new(),
        target: Some(TargetKeyInputs {
            resolved_commit: CommitSha::new("b".repeat(40)).unwrap(),
            translations_blake3: "t".to_owned(),
            walk_limit: 5000,
            already_present_enabled: true,
        }),
        config_blake3: "cfg".to_owned(),
        schema_version: 12,
        crate_version: "0.17.0".to_owned(),
    }
}

// The guard the whole measurement loop rests on: an upgrade must not
// change the fingerprint, or a months-old outcome joins nothing.
#[test]
fn fingerprint_is_stable_across_crate_and_schema_versions() {
    let a = key().fingerprint("patch").unwrap();
    let mut newer = key();
    newer.crate_version = "0.18.0".to_owned();
    newer.schema_version = 13;
    let b = newer.fingerprint("patch").unwrap();
    assert_eq!(a, b);
}

// A cherry-pick hop re-mints the SHA but not the patch; both must
// share a fingerprint so their outcomes join.
#[test]
fn fingerprint_ignores_the_commit_sha() {
    let a = key().fingerprint("patch").unwrap();
    let mut moved = key();
    moved.commit = CommitSha::new("c".repeat(40)).unwrap();
    let b = moved.fingerprint("patch").unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_different_patch_changes_the_fingerprint() {
    let a = key().fingerprint("patch-a").unwrap();
    let b = key().fingerprint("patch-b").unwrap();
    assert_ne!(a, b);
}

#[test]
fn a_moved_target_tip_changes_the_fingerprint() {
    let a = key().fingerprint("patch").unwrap();
    let mut moved = key();
    moved.target.as_mut().unwrap().resolved_commit = CommitSha::new("d".repeat(40)).unwrap();
    let b = moved.fingerprint("patch").unwrap();
    assert_ne!(a, b);
}

// A regenerated snapshot changes the verdict, so it must change the
// fingerprint too.
#[test]
fn a_regenerated_snapshot_changes_the_fingerprint() {
    let a = key().fingerprint("patch").unwrap();
    let mut other = key();
    other.pins = vec![pin("snap-b")];
    let b = other.fingerprint("patch").unwrap();
    assert_ne!(a, b);
}

#[test]
fn a_different_config_changes_the_fingerprint() {
    let a = key().fingerprint("patch").unwrap();
    let mut other = key();
    other.config_blake3 = "cfg2".to_owned();
    let b = other.fingerprint("patch").unwrap();
    assert_ne!(a, b);
}

#[test]
fn identical_inputs_fingerprint_identically() {
    assert_eq!(key().fingerprint("patch"), key().fingerprint("patch"));
}
