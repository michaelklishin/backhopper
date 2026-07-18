// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::state::Canonical;
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SnapshotHeader};

use backhopper_cli::commands::snapshots::compute_diff;

fn header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "proptest".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

fn build(modules: Vec<(String, Vec<(String, u8)>)>) -> Snapshot<Canonical> {
    let mods: Vec<Module> = modules
        .into_iter()
        .map(|(name, exports)| {
            let mut m = Module::new(ModuleName::new(name).unwrap());
            for (n, a) in exports {
                m.exports.push(FunArity {
                    name: FunctionName::new(n).unwrap(),
                    arity: Arity::new(a),
                });
            }
            m
        })
        .collect();
    Snapshot::from_extracted(header("p", "v0.1.0"), mods, vec![]).into_canonical()
}

fn arb_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,4}"
}

fn arb_module_spec() -> impl Strategy<Value = (String, Vec<(String, u8)>)> {
    (
        arb_atom(),
        prop::collection::vec((arb_atom(), 0u8..=4), 0..3),
    )
}

proptest! {
    #[test]
    fn equal_snapshots_have_empty_diff(
        modules in prop::collection::vec(arb_module_spec(), 0..3),
    ) {
        let s = build(modules);
        let d = compute_diff(&s, &s);
        prop_assert!(d.modules_added.is_empty());
        prop_assert!(d.modules_removed.is_empty());
        prop_assert!(d.exports_added.is_empty());
        prop_assert!(d.exports_removed.is_empty());
    }

    #[test]
    fn diff_swapped_inputs_inverts_added_and_removed(
        a_modules in prop::collection::vec(arb_module_spec(), 0..3),
        b_modules in prop::collection::vec(arb_module_spec(), 0..3),
    ) {
        let a = build(a_modules);
        let b = build(b_modules);
        let d_ab = compute_diff(&a, &b);
        let d_ba = compute_diff(&b, &a);
        let added_ab: BTreeSet<_> = d_ab.modules_added.into_iter().collect();
        let removed_ba: BTreeSet<_> = d_ba.modules_removed.into_iter().collect();
        prop_assert_eq!(added_ab, removed_ba);
    }

    #[test]
    fn diff_added_plus_removed_covers_symmetric_difference(
        a_modules in prop::collection::vec(arb_module_spec(), 0..3),
        b_modules in prop::collection::vec(arb_module_spec(), 0..3),
    ) {
        let a = build(a_modules.clone());
        let b = build(b_modules.clone());
        let d = compute_diff(&a, &b);
        let a_names: BTreeSet<&str> = a.modules().iter().map(|m| m.name.as_str()).collect();
        let b_names: BTreeSet<&str> = b.modules().iter().map(|m| m.name.as_str()).collect();
        let symmetric: BTreeSet<String> = a_names
            .symmetric_difference(&b_names)
            .map(|s| (*s).to_owned())
            .collect();
        let mut diff_total: BTreeSet<String> =
            d.modules_added.iter().map(|m| m.as_str().to_owned()).collect();
        diff_total.extend(d.modules_removed.iter().map(|m| m.as_str().to_owned()));
        prop_assert_eq!(symmetric, diff_total);
    }
}
