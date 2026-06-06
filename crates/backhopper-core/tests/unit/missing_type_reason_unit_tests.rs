// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName, TypeName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, Module, Snapshot, SnapshotHeader, TypeArity, Visibility, state,
};
use backhopper_core::model::verdict::{Reason, Verdict};

fn header(project: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

fn module_named(name: &str) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.visibility = Visibility::Public;
    m
}

fn with_exported_type(mut m: Module, name: &str, arity: u8) -> Module {
    m.export_types.push(TypeArity {
        name: TypeName::new(name).unwrap(),
        arity: Arity::new(arity),
    });
    m
}

fn with_exported_function(mut m: Module, name: &str, arity: u8) -> Module {
    m.exports.push(FunArity {
        name: FunctionName::new(name).unwrap(),
        arity: Arity::new(arity),
    });
    m
}

fn snapshot(project: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header(project), modules, vec![]).into_canonical()
}

fn context(project: &str, modules: Vec<Module>) -> EvaluationContext {
    let pin = Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    );
    let snap = snapshot(project, modules);
    let scope = PinScope::from_snapshot(ProjectName::new(project).unwrap(), &snap, []);
    EvaluationContext::new(pin, snap, scope)
}

const VARIANT_B_DIFF: &str = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,4 @@
 -module(x).
+-spec init([ra:index()]) -> ok.
+init(Servers) ->
+    ok.
";

#[test]
fn exported_type_is_compatible() {
    let m = with_exported_type(module_named("ra"), "index", 0);
    let eval = Patch::parse(VARIANT_B_DIFF.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context("ra", vec![m])]);
    let r0 = &eval.verdict.results[0];
    assert!(
        matches!(r0.verdict, Verdict::Compatible),
        "verdict was {:?}",
        r0.verdict
    );
}

#[test]
fn unexported_type_emits_missing_type_reason() {
    // ra exports `start/0` (a function) but no types.
    let m = with_exported_function(module_named("ra"), "start", 0);
    let eval = Patch::parse(VARIANT_B_DIFF.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context("ra", vec![m])]);
    let r0 = &eval.verdict.results[0];
    let reasons = r0.verdict.reasons();
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            Reason::MissingType { module, name, arity }
                if module.as_str() == "ra"
                    && name.as_str() == "index"
                    && arity.get() == 0
        )),
        "expected MissingType reason, got {reasons:?}"
    );
}

#[test]
fn missing_type_is_not_blocking_so_verdict_is_requires_adaptation_not_incompatible() {
    let m = with_exported_function(module_named("ra"), "start", 0);
    let eval = Patch::parse(VARIANT_B_DIFF.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context("ra", vec![m])]);
    let r0 = &eval.verdict.results[0];
    assert!(
        matches!(r0.verdict, Verdict::RequiresAdaptation { .. }),
        "MissingType alone should yield RequiresAdaptation, got {:?}",
        r0.verdict
    );
}

#[test]
fn function_call_to_same_name_still_emits_missing_symbol_in_body() {
    let body_diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:index().
";
    let m = with_exported_function(module_named("ra"), "start", 0);
    let eval = Patch::parse(body_diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context("ra", vec![m])]);
    let r0 = &eval.verdict.results[0];
    let reasons = r0.verdict.reasons();
    // In body context the same `ra:index()` is a function call; it
    // routes to MissingSymbol, not MissingType.
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::MissingSymbol { .. })),
        "expected MissingSymbol in body context, got {reasons:?}"
    );
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::MissingType { .. })),
        "MissingType must not fire in body context, got {reasons:?}"
    );
    assert!(
        matches!(r0.verdict, Verdict::Incompatible { .. }),
        "verdict was {:?}",
        r0.verdict
    );
}

#[test]
fn out_of_scope_type_ref_produces_no_reason() {
    let m = with_exported_type(module_named("ra"), "index", 0);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,3 @@
 -module(x).
+-spec foo(unknown:t()) -> ok.
+foo(_) -> ok.
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context("ra", vec![m])]);
    let r0 = &eval.verdict.results[0];
    let reasons = r0.verdict.reasons();
    assert!(
        reasons.is_empty(),
        "out-of-scope type ref should not produce reasons, got {reasons:?}"
    );
}
