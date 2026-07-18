// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{ModuleName, ProjectName, TagName};
use backhopper_core::model::snapshot_diff::{DiffPayload, QualifiedFunArity};

fn empty_diff() -> DiffPayload {
    DiffPayload {
        project: ProjectName::new("ra").unwrap(),
        from: TagName::new("v2.0.0").unwrap(),
        to: TagName::new("v2.1.0").unwrap(),
        modules_added: Vec::new(),
        modules_removed: Vec::new(),
        exports_added: Vec::new(),
        exports_removed: Vec::new(),
        types_added: Vec::new(),
        types_removed: Vec::new(),
        callbacks_added: Vec::new(),
        callbacks_removed: Vec::new(),
        headers_added: Vec::new(),
        headers_removed: Vec::new(),
        records_added: Vec::new(),
        records_removed: Vec::new(),
        versioned_machine_version_changes: Vec::new(),
        wire_constant_changes: Vec::new(),
    }
}

#[test]
fn is_empty_true_when_no_axis_changed() {
    assert!(empty_diff().is_empty());
}

#[test]
fn is_empty_false_when_any_axis_changed() {
    let mut d = empty_diff();
    d.modules_added.push(ModuleName::new("ra_server").unwrap());
    assert!(!d.is_empty());
}

#[test]
fn breaking_removal_count_sums_only_removed_axes() {
    let mut d = empty_diff();
    d.modules_removed.push(ModuleName::new("ra_gone").unwrap());
    d.exports_removed.push(QualifiedFunArity {
        module: ModuleName::new("ra_server").unwrap(),
        fun_arity: "start/1".into(),
    });
    // an added export is not a breaking removal
    d.exports_added.push(QualifiedFunArity {
        module: ModuleName::new("ra_server").unwrap(),
        fun_arity: "start/2".into(),
    });
    assert_eq!(d.breaking_removal_count(), 2);
}

// The name newtypes are serde-transparent: the wire form is the same bare-string JSON.
#[test]
fn name_newtypes_serialize_as_bare_strings() {
    let mut d = empty_diff();
    d.modules_added.push(ModuleName::new("ra_server").unwrap());
    let json = serde_json::to_string(&d).unwrap();
    assert!(json.contains("\"project\":\"ra\""), "{json}");
    assert!(json.contains("\"from\":\"v2.0.0\""), "{json}");
    assert!(json.contains("\"to\":\"v2.1.0\""), "{json}");
    assert!(json.contains("\"modules_added\":[\"ra_server\"]"), "{json}");
}

#[test]
fn payload_round_trips_through_json() {
    let mut d = empty_diff();
    d.modules_removed.push(ModuleName::new("ra_gone").unwrap());
    let json = serde_json::to_string(&d).unwrap();
    let back: DiffPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}
