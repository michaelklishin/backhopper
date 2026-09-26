// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{Arity, FunctionName, TagName};
use backhopper_core::model::snapshot::{
    ArityMatch, Deprecation, DeprecationReplacement, Module, Snapshot, Visibility, state,
};
use backhopper_core::model::verdict::{Reason, SeriesVerdict, Verdict};
use backhopper_test_support::{canonical_snapshot, module_with, pin, snapshot_header};

fn module(name: &str, visibility: Visibility, exports: &[(&str, u8)]) -> Module {
    let mut m = module_with(name, exports);
    m.visibility = visibility;
    m
}

fn snapshot_with(modules: Vec<Module>) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("ra", "v3.1.6"), modules)
}

fn evaluate(diff: &str, snap: Snapshot<state::Canonical>) -> SeriesVerdict {
    evaluate_with_source(diff, snap, None)
}

fn evaluate_with_source(
    diff: &str,
    snap: Snapshot<state::Canonical>,
    source: Option<Snapshot<state::Canonical>>,
) -> SeriesVerdict {
    let target = pin("ra", "v3.1.6");
    let scope = PinScope::from_snapshot(target.project.clone(), &snap, Vec::new());
    let mut ctx = EvaluationContext::for_pin(target, snap).with_scope(scope);
    if let Some(source) = source {
        ctx = ctx.with_source_snapshot(source);
    }
    Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx])
        .verdict
}

const CALLS_RA_INTERNAL_INIT_1: &str = "\
diff --git a/ra_server.erl b/ra_server.erl
--- a/ra_server.erl
+++ b/ra_server.erl
@@ -1,1 +1,2 @@
 -module(ra_server).
+apply() -> ra_internal:init(1).
";

const CALLS_RA_INTERNAL_INIT_WRAPPED: &str = "\
diff --git a/ra_server.erl b/ra_server.erl
--- a/ra_server.erl
+++ b/ra_server.erl
@@ -1,1 +1,2 @@
 -module(ra_server).
+apply(A) -> ra_internal:init(A,
";

fn ra_internal(visibility: Visibility, exports: &[(&str, u8)]) -> Snapshot<state::Canonical> {
    snapshot_with(vec![module("ra_internal", visibility, exports)])
}

fn has_now_hidden(verdict: &Verdict) -> bool {
    verdict
        .reasons()
        .iter()
        .any(|r| matches!(r, Reason::NowHidden { module } if module.as_str() == "ra_internal"))
}

#[test]
fn a_call_into_a_module_hidden_on_both_sides_has_no_now_hidden() {
    for diff in [CALLS_RA_INTERNAL_INIT_1, CALLS_RA_INTERNAL_INIT_WRAPPED] {
        let v = evaluate_with_source(
            diff,
            ra_internal(Visibility::Hidden, &[("init", 1)]),
            Some(ra_internal(Visibility::Hidden, &[("init", 1)])),
        );
        assert!(
            matches!(v.results[0].verdict, Verdict::Compatible),
            "{diff}"
        );
    }
}

#[test]
fn a_call_into_a_module_hidden_only_on_the_target_is_requires_adaptation() {
    for diff in [CALLS_RA_INTERNAL_INIT_1, CALLS_RA_INTERNAL_INIT_WRAPPED] {
        let v = evaluate_with_source(
            diff,
            ra_internal(Visibility::Hidden, &[("init", 1)]),
            Some(ra_internal(Visibility::Public, &[("init", 1)])),
        );
        let verdict = &v.results[0].verdict;
        assert!(
            matches!(verdict, Verdict::RequiresAdaptation { .. }),
            "{diff}"
        );
        assert!(has_now_hidden(verdict), "{diff}");
    }
}

#[test]
fn now_hidden_needs_a_source_snapshot() {
    for diff in [CALLS_RA_INTERNAL_INIT_1, CALLS_RA_INTERNAL_INIT_WRAPPED] {
        let v = evaluate(diff, ra_internal(Visibility::Hidden, &[("init", 1)]));
        assert!(
            matches!(v.results[0].verdict, Verdict::Compatible),
            "{diff}"
        );
    }
}

#[test]
fn a_missing_function_in_a_hidden_module_is_missing_symbol() {
    for diff in [CALLS_RA_INTERNAL_INIT_1, CALLS_RA_INTERNAL_INIT_WRAPPED] {
        let v = evaluate_with_source(
            diff,
            ra_internal(Visibility::Hidden, &[("start", 0)]),
            Some(ra_internal(Visibility::Public, &[("init", 1)])),
        );
        let verdict = &v.results[0].verdict;
        assert!(matches!(verdict, Verdict::Incompatible { .. }), "{diff}");
        assert!(
            verdict
                .reasons()
                .iter()
                .any(|r| matches!(r, Reason::MissingSymbol { .. })),
            "{diff}"
        );
        assert!(has_now_hidden(verdict), "{diff}");
    }
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
    let v = evaluate(diff, snap);
    let r0 = &v.results[0];
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::DeprecatedUsage { .. }))
    );
}

#[test]
fn deprecated_usage_carries_since_and_replacement_from_the_snapshot() {
    let mut m = module("ra", Visibility::Public, &[("start_node", 2)]);
    m.deprecations.push(Deprecation {
        function: Some(FunctionName::new("start_node").unwrap()),
        arity_match: ArityMatch::Exact {
            arity: Arity::new(2),
        },
        since: Some(TagName::new("v2.0.0").unwrap()),
        replacement: Some(DeprecationReplacement {
            function: FunctionName::new("start").unwrap(),
            arity: Arity::new(2),
        }),
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
    let v = evaluate(diff, snap);
    let (since, replacement) = v.results[0]
        .verdict
        .reasons()
        .iter()
        .find_map(|r| match r {
            Reason::DeprecatedUsage {
                since, replacement, ..
            } => Some((since.clone(), replacement.clone())),
            _ => None,
        })
        .expect("deprecated usage reason");
    assert_eq!(since.map(|t| t.to_string()), Some("v2.0.0".to_owned()));
    // the replacement resolves to ra:start/2 in the same module
    let replacement = replacement.expect("replacement filled from the snapshot");
    assert!(format!("{replacement:?}").contains("start"));
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
    let v = evaluate(diff, snap);
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
    let v = evaluate(diff, snap);
    let r0 = &v.results[0];
    assert!(r0.verdict.is_compatible() || matches!(r0.verdict, Verdict::Compatible));
}
