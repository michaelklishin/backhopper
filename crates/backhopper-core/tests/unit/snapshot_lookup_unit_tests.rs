// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::{Module, Snapshot, state};
use backhopper_test_support::{canonical_snapshot, module_with, snapshot_header};

fn canonical_with(modules: Vec<Module>) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("ra", "v0.1.0"), modules)
}

#[test]
fn module_named_returns_target_module() {
    let snap = canonical_with(vec![
        module_with("alpha", &[("a", 0)]),
        module_with("beta", &[("b", 1)]),
        module_with("gamma", &[("c", 2)]),
    ]);
    let found = snap
        .module_named(&ModuleName::new("beta").unwrap())
        .unwrap();
    assert_eq!(found.name.as_str(), "beta");
    assert_eq!(found.exports.len(), 1);
}

#[test]
fn module_named_returns_none_for_absent() {
    let snap = canonical_with(vec![module_with("alpha", &[])]);
    assert!(
        snap.module_named(&ModuleName::new("zeta").unwrap())
            .is_none()
    );
}

#[test]
fn module_named_handles_first_and_last_alphabetical_slots() {
    let snap = canonical_with(vec![
        module_with("aaa", &[]),
        module_with("mid", &[]),
        module_with("zzz", &[]),
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
    let snap = canonical_with(vec![module_with("m", &[("f", 1), ("f", 2)])]);
    let m = ModuleName::new("m").unwrap();
    let f = FunctionName::new("f").unwrap();
    assert!(snap.lookup_export(&m, &f, Arity::new(1)));
    assert!(snap.lookup_export(&m, &f, Arity::new(2)));
}

#[test]
fn lookup_export_rejects_wrong_arity() {
    let snap = canonical_with(vec![module_with("m", &[("f", 1)])]);
    let m = ModuleName::new("m").unwrap();
    let f = FunctionName::new("f").unwrap();
    assert!(!snap.lookup_export(&m, &f, Arity::new(0)));
    assert!(!snap.lookup_export(&m, &f, Arity::new(2)));
}

#[test]
fn lookup_export_rejects_unknown_module() {
    let snap = canonical_with(vec![module_with("m", &[("f", 1)])]);
    let other = ModuleName::new("other").unwrap();
    let f = FunctionName::new("f").unwrap();
    assert!(!snap.lookup_export(&other, &f, Arity::new(1)));
}
