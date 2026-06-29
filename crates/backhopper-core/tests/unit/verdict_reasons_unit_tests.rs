// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::Patch;
use backhopper_core::model::names::{Arity, FunctionName};
use backhopper_core::model::snapshot::{
    ArityMatch, Deprecation, Module, Snapshot, Visibility, state,
};
use backhopper_core::model::verdict::{Reason, Verdict};
use backhopper_test_support::{canonical_snapshot, module_with, pin, snapshot_header};

fn module(name: &str, visibility: Visibility, exports: &[(&str, u8)]) -> Module {
    let mut m = module_with(name, exports);
    m.visibility = visibility;
    m
}

fn snapshot_with(modules: Vec<Module>) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("ra", "v3.1.6"), modules)
}

#[test]
fn reference_to_hidden_module_is_now_hidden() {
    let snap = snapshot_with(vec![module(
        "ra_internal",
        Visibility::Hidden,
        &[("init", 1)],
    )]);
    let diff = "\
diff --git a/ra_server.erl b/ra_server.erl
--- a/ra_server.erl
+++ b/ra_server.erl
@@ -1,1 +1,2 @@
 -module(ra_server).
+apply() -> ra_internal:init(1).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin("ra", "v3.1.6"), snap)]);
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
diff --git a/ra_directory.erl b/ra_directory.erl
--- a/ra_directory.erl
+++ b/ra_directory.erl
@@ -1,1 +1,2 @@
 -module(ra_directory).
+register_name() -> ra:start_node(Server, Cmd).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin("ra", "v3.1.6"), snap)]);
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
diff --git a/ra_server_proc.erl b/ra_server_proc.erl
--- a/ra_server_proc.erl
+++ b/ra_server_proc.erl
@@ -1,1 +1,2 @@
 -module(ra_server_proc).
+handle_command() -> ra:process_command(Server, Cmd).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin("ra", "v3.1.6"), snap)]);
    let r0 = &v.results[0];
    assert!(
        r0.verdict.reasons().iter().any(
            |r| matches!(r, Reason::ArityChanged { expected, .. } if *expected == Arity::new(2))
        )
    );
}

#[test]
fn helper_defined_in_patch_is_not_missing() {
    let snap = snapshot_with(vec![module("ra_lib", Visibility::Public, &[("id", 1)])]);
    let diff = "\
diff --git a/ra_machine.erl b/ra_machine.erl
--- a/ra_machine.erl
+++ b/ra_machine.erl
@@ -1,1 +1,4 @@
 -module(ra_machine).
+helper(X) -> X + 1.
+apply() -> _local:helper(2).
";
    let v = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against_series(&[(pin("ra", "v3.1.6"), snap)]);
    let r0 = &v.results[0];
    assert!(r0.verdict.is_compatible() || matches!(r0.verdict, Verdict::Compatible));
}
