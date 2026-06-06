// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{MacroName, ModuleName};
use backhopper_core::model::verdict::{Reason, SnapshotSide};

fn module(name: &str) -> ModuleName {
    ModuleName::new(name).unwrap()
}

fn macro_name(name: &str) -> MacroName {
    MacroName::new(name).unwrap()
}

#[test]
fn versioned_machine_snapshot_missing_is_non_blocking() {
    let r = Reason::VersionedMachineSnapshotMissing {
        module: module("rabbit_fifo"),
        side: SnapshotSide::Both,
    };
    assert!(!r.is_blocking());
}

#[test]
fn wire_constant_bindings_missing_is_non_blocking() {
    let r = Reason::WireConstantBindingsMissing {
        module: module("ra_log_segment"),
        macros: vec![macro_name("MAGIC"), macro_name("VERSION")],
        side: SnapshotSide::Source,
    };
    assert!(!r.is_blocking());
}

#[test]
fn versioned_machine_snapshot_missing_round_trips_through_serde_for_each_side() {
    for side in [
        SnapshotSide::Source,
        SnapshotSide::Target,
        SnapshotSide::Both,
    ] {
        let r = Reason::VersionedMachineSnapshotMissing {
            module: module("rabbit_fifo"),
            side,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: Reason = serde_json::from_str(&j).unwrap();
        assert_eq!(r, back, "round trip failed for side {side:?}");
    }
}

#[test]
fn wire_constant_bindings_missing_round_trips_through_serde() {
    let r = Reason::WireConstantBindingsMissing {
        module: module("ra_log_segment"),
        macros: vec![macro_name("MAGIC"), macro_name("VERSION")],
        side: SnapshotSide::Target,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: Reason = serde_json::from_str(&j).unwrap();
    assert_eq!(r, back);
}

#[test]
fn versioned_machine_snapshot_missing_serializes_with_snake_case_kind_tag() {
    let r = Reason::VersionedMachineSnapshotMissing {
        module: module("rabbit_fifo"),
        side: SnapshotSide::Both,
    };
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["kind"], "versioned_machine_snapshot_missing");
    assert_eq!(v["side"], "both");
}

#[test]
fn wire_constant_bindings_missing_serializes_with_snake_case_kind_tag() {
    let r = Reason::WireConstantBindingsMissing {
        module: module("ra_log_segment"),
        macros: vec![macro_name("VERSION")],
        side: SnapshotSide::Source,
    };
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["kind"], "wire_constant_bindings_missing");
    assert_eq!(v["side"], "source");
    assert_eq!(v["macros"][0], "VERSION");
}

#[test]
fn snapshot_side_serializes_as_snake_case() {
    let s = serde_json::to_value(SnapshotSide::Source).unwrap();
    let t = serde_json::to_value(SnapshotSide::Target).unwrap();
    let b = serde_json::to_value(SnapshotSide::Both).unwrap();
    assert_eq!(s, "source");
    assert_eq!(t, "target");
    assert_eq!(b, "both");
}
