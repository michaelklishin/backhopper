// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::compat::patch::Patch;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    ArityMatch, Deprecation, FunArity, Module, Snapshot, SnapshotHeader, Visibility, state,
};
use backhopper_core::model::verdict::{Reason, Verdict};

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v3.1.6").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
    }
}

fn module(name: &str, visibility: Visibility, exports: &[(&str, u8)]) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.visibility = visibility;
    for (n, a) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(*n).unwrap(),
            arity: Arity::new(*a),
        });
    }
    m
}

fn snapshot_with(modules: Vec<Module>) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header(), modules, vec![]).into_canonical()
}

fn pin() -> Pin {
    Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v3.1.6").unwrap(),
    )
}

#[test]
fn reference_to_hidden_module_is_now_hidden() {
    let snap = snapshot_with(vec![module(
        "ra_internal",
        Visibility::Hidden,
        &[("foo", 1)],
    )]);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra_internal:foo(1).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin(), snap)]);
    let r0 = &v.results[0];
    assert!(matches!(
        r0.verdict,
        Verdict::Incompatible { .. } | Verdict::RequiresAdaptation { .. }
    ));
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::NowHidden { module } if module.as_str() == "ra_internal"))
    );
}

#[test]
fn reference_to_deprecated_function_yields_requires_adaptation() {
    let mut m = module("ra", Visibility::Public, &[("start_node", 2)]);
    m.deprecations.push(Deprecation {
        function: Some(FunctionName::new("start_node").unwrap()),
        arity_match: ArityMatch::Exact {
            arity: Arity::new(2),
        },
        since: None,
        replacement: None,
        reason: None,
        module_wide: false,
    });
    let snap = snapshot_with(vec![m]);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:start_node(a, b).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin(), snap)]);
    let r0 = &v.results[0];
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::DeprecatedUsage { .. }))
    );
}

#[test]
fn arity_change_is_distinct_from_missing_symbol() {
    let snap = snapshot_with(vec![module(
        "ra",
        Visibility::Public,
        &[("process_command", 3)],
    )]);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:process_command(a, b).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin(), snap)]);
    let r0 = &v.results[0];
    assert!(
        r0.verdict.reasons().iter().any(
            |r| matches!(r, Reason::ArityChanged { expected, .. } if *expected == Arity::new(2))
        )
    );
}

#[test]
fn helper_defined_in_patch_is_not_missing() {
    let snap = snapshot_with(vec![module("foo", Visibility::Public, &[("bar", 1)])]);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,4 @@
 -module(x).
+helper(X) -> X + 1.
+go() -> _local:helper(2).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin(), snap)]);
    let r0 = &v.results[0];
    assert!(r0.verdict.is_compatible() || matches!(r0.verdict, Verdict::Compatible));
}