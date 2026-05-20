// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SnapshotHeader};

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v0.1.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
    }
}

fn mk_module(name: &str, exports: &[(&str, u8)]) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    for (f, a) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(*f).unwrap(),
            arity: Arity::new(*a),
        });
    }
    m
}

fn canonical_with(
    modules: Vec<Module>,
) -> Snapshot<backhopper_core::model::snapshot::state::Canonical> {
    Snapshot::from_extracted(header(), modules, Vec::new()).into_canonical()
}

#[test]
fn module_named_returns_target_module() {
    let snap = canonical_with(vec![
        mk_module("alpha", &[("a", 0)]),
        mk_module("beta", &[("b", 1)]),
        mk_module("gamma", &[("c", 2)]),
    ]);
    let found = snap
        .module_named(&ModuleName::new("beta").unwrap())
        .unwrap();
    assert_eq!(found.name.as_str(), "beta");
    assert_eq!(found.exports.len(), 1);
}

#[test]
fn module_named_returns_none_for_absent() {
    let snap = canonical_with(vec![mk_module("alpha", &[])]);
    assert!(
        snap.module_named(&ModuleName::new("zeta").unwrap())
            .is_none()
    );
}

#[test]
fn module_named_handles_first_and_last_alphabetical_slots() {
    let snap = canonical_with(vec![
        mk_module("aaa", &[]),
        mk_module("mid", &[]),
        mk_module("zzz", &[]),
    ]);
    assert!(
        snap.module_named(&ModuleName::new("aaa").unwrap())
            .is_some()
    );
    assert!(
        snap.module_named(&ModuleName::new("zzz").unwrap())
            .is_some()
    );
}

#[test]
fn lookup_export_matches_existing_function_and_arity() {
    let snap = canonical_with(vec![mk_module("m", &[("f", 1), ("f", 2)])]);
    let m = ModuleName::new("m").unwrap();
    let f = FunctionName::new("f").unwrap();
    assert!(snap.lookup_export(&m, &f, Arity::new(1)));
    assert!(snap.lookup_export(&m, &f, Arity::new(2)));
}

#[test]
fn lookup_export_rejects_wrong_arity() {
    let snap = canonical_with(vec![mk_module("m", &[("f", 1)])]);
    let m = ModuleName::new("m").unwrap();
    let f = FunctionName::new("f").unwrap();
    assert!(!snap.lookup_export(&m, &f, Arity::new(0)));
    assert!(!snap.lookup_export(&m, &f, Arity::new(2)));
}

#[test]
fn lookup_export_rejects_unknown_module() {
    let snap = canonical_with(vec![mk_module("m", &[("f", 1)])]);
    let other = ModuleName::new("other").unwrap();
    let f = FunctionName::new("f").unwrap();
    assert!(!snap.lookup_export(&other, &f, Arity::new(1)));
}