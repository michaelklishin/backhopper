// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::state::Canonical;
use backhopper_core::model::snapshot::{
    FORMAT_VERSION, FunArity, Module, Snapshot, SnapshotHeader,
};

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v3.1.6").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
        generated_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        extractor_version: String::new(),
        format_version: FORMAT_VERSION,
        dep_pins: Vec::new(),
    }
}

fn module_named(name: &str) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
    });
    m
}

#[test]
fn a_canonical_snapshot_round_trips_through_json() {
    let snap =
        Snapshot::from_extracted(header(), vec![module_named("ra_log")], vec![]).into_canonical();
    let json = serde_json::to_string(&snap).unwrap();
    let back: Snapshot<Canonical> = serde_json::from_str(&json).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn an_out_of_order_module_list_is_not_canonical() {
    let snap = Snapshot::from_extracted(
        header(),
        vec![module_named("ra_server"), module_named("ra_log")],
        vec![],
    )
    .into_canonical();
    let mut json: serde_json::Value = serde_json::to_value(&snap).unwrap();
    let modules = json.get_mut("modules").unwrap().as_array_mut().unwrap();
    modules.reverse();
    let err = serde_json::from_value::<Snapshot<Canonical>>(json).unwrap_err();
    assert!(
        err.to_string().contains("modules out of order at ra_log"),
        "{err}"
    );
}

// serde_json refuses a map keyed by a struct; clause_heads is keyed by
// FunArity, so the map travels through its Display form instead.
#[test]
fn a_module_with_a_clause_head_serializes_to_json() {
    let mut m = module_named("ra_log");
    let fa = FunArity {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
    };
    m.clause_heads
        .insert(fa.clone(), vec![vec![ArgShape::Variable]]);
    let snap = Snapshot::from_extracted(header(), vec![m], vec![]).into_canonical();
    let json = serde_json::to_string(&snap).unwrap();
    assert!(json.contains("\"init/1\""), "{json}");
    let back: Snapshot<Canonical> = serde_json::from_str(&json).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn a_duplicate_module_name_is_not_canonical() {
    let snap =
        Snapshot::from_extracted(header(), vec![module_named("ra_log")], vec![]).into_canonical();
    let mut json: serde_json::Value = serde_json::to_value(&snap).unwrap();
    let modules = json.get_mut("modules").unwrap().as_array_mut().unwrap();
    let dup = modules[0].clone();
    modules.push(dup);
    let result: Result<Snapshot<Canonical>, _> = serde_json::from_value(json);
    assert!(result.is_err());
}
