// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::config::{FamilyDefaults, OptionTypeDecl};
use backhopper_core::model::names::{Arity, ModuleName, ProjectName, TagName, TypeName};
use backhopper_core::model::snapshot::{Module, Snapshot, TypeDecl, state};
use backhopper_core::model::verdict::{Reason, Verdict};
use backhopper_test_support::{canonical_snapshot, snapshot_header};

fn opts_module(rhs: &str) -> Module {
    let mut m = Module::new(ModuleName::new("cowboy_websocket").unwrap());
    m.types.push(TypeDecl {
        name: TypeName::new("opts").unwrap(),
        arity: Arity::new(0),
        rhs: rhs.to_owned(),
    });
    m
}

fn canonical(tag: &str, rhs: &str) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("cowboy", tag), vec![opts_module(rhs)])
}

fn option_defaults() -> FamilyDefaults {
    FamilyDefaults {
        option_types: vec![OptionTypeDecl::new("cowboy_websocket", "opts").unwrap()],
        ..Default::default()
    }
}

fn evaluate(
    diff: &[u8],
    target: Snapshot<state::Canonical>,
    source: Option<Snapshot<state::Canonical>>,
) -> Vec<Reason> {
    let pin = make_pin();
    let scope = PinScope::from_snapshot(ProjectName::new("cowboy").unwrap(), &target, Vec::new());
    let mut ctx = EvaluationContext::for_pin(pin, target)
        .with_scope(scope)
        .with_files(EvaluationFiles::new())
        .with_family_defaults(option_defaults());
    if let Some(source) = source {
        ctx = ctx.with_source_snapshot(source);
    }
    let patch = Patch::parse(diff).unwrap().analyze();
    let series = patch.evaluate_series(&[ctx]);
    match &series.verdict.results[0].verdict {
        Verdict::Compatible | Verdict::Inapplicable { .. } => Vec::new(),
        Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
            reasons.clone()
        }
    }
}

fn make_pin() -> backhopper_core::model::pin::Pin {
    backhopper_core::model::pin::Pin::new(
        ProjectName::new("cowboy").unwrap(),
        TagName::new("target").unwrap(),
    )
}

const TARGET_RHS: &str = "#{idle_timeout := integer()}";
const SOURCE_RHS: &str = "#{idle_timeout := integer(), data_delivery => atom()}";

const ADD_DATA_DELIVERY: &[u8] = b"\
diff --git a/h.erl b/h.erl
--- a/h.erl
+++ b/h.erl
@@ -1,1 +1,2 @@
 f() -> ok.
+g() -> #{data_delivery => relay}.
";

#[test]
fn drift_fires_for_data_delivery_against_older_target() {
    let target = canonical("2.13.0", TARGET_RHS);
    let source = canonical("2.14.0", SOURCE_RHS);
    let reasons = evaluate(ADD_DATA_DELIVERY, target, Some(source));
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::OptionKeyUnknownOnPin { key, .. } if key.as_str() == "data_delivery")),
        "expected a finding, got {reasons:?}"
    );
}

#[test]
fn application_map_key_never_fires() {
    let target = canonical("2.13.0", TARGET_RHS);
    let source = canonical("2.14.0", SOURCE_RHS);
    let diff: &[u8] = b"\
diff --git a/h.erl b/h.erl
--- a/h.erl
+++ b/h.erl
@@ -1,1 +1,2 @@
 f() -> ok.
+g() -> #{queue => q1}.
";
    let reasons = evaluate(diff, target, Some(source));
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn key_on_both_sides_never_fires() {
    let target = canonical("2.13.0", TARGET_RHS);
    let source = canonical("2.14.0", SOURCE_RHS);
    let diff: &[u8] = b"\
diff --git a/h.erl b/h.erl
--- a/h.erl
+++ b/h.erl
@@ -1,1 +1,2 @@
 f() -> ok.
+g() -> #{idle_timeout => 5}.
";
    let reasons = evaluate(diff, target, Some(source));
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn no_source_side_means_silence() {
    let target = canonical("2.13.0", TARGET_RHS);
    let reasons = evaluate(ADD_DATA_DELIVERY, target, None);
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn added_spec_line_does_not_fire() {
    let target = canonical("2.13.0", TARGET_RHS);
    let source = canonical("2.14.0", SOURCE_RHS);
    let diff: &[u8] = b"\
diff --git a/h.erl b/h.erl
--- a/h.erl
+++ b/h.erl
@@ -1,1 +1,2 @@
 f() -> ok.
+-spec start(#{data_delivery => atom()}) -> ok.
";
    let reasons = evaluate(diff, target, Some(source));
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn quoted_atom_key_fires() {
    let target = canonical("2.13.0", TARGET_RHS);
    let source = canonical("2.14.0", SOURCE_RHS);
    let diff: &[u8] = b"\
diff --git a/h.erl b/h.erl
--- a/h.erl
+++ b/h.erl
@@ -1,1 +1,2 @@
 f() -> ok.
+g() -> #{'data_delivery' => relay}.
";
    let reasons = evaluate(diff, target, Some(source));
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::OptionKeyUnknownOnPin { .. })),
        "expected a finding, got {reasons:?}"
    );
}

#[test]
fn open_target_set_suppresses() {
    let target = canonical("2.13.0", "#{idle_timeout := integer(), atom() => any()}");
    let source = canonical("2.14.0", SOURCE_RHS);
    let reasons = evaluate(ADD_DATA_DELIVERY, target, Some(source));
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn one_reason_per_type_key_pair() {
    let target = canonical("2.13.0", TARGET_RHS);
    let source = canonical("2.14.0", SOURCE_RHS);
    let diff: &[u8] = b"\
diff --git a/h.erl b/h.erl
--- a/h.erl
+++ b/h.erl
@@ -1,1 +1,4 @@
 f() -> ok.
+g() -> #{data_delivery => relay}.
+h() -> #{data_delivery => stream_handlers}.
+i() -> #{data_delivery => relay}.
";
    let reasons = evaluate(diff, target, Some(source));
    let count = reasons
        .iter()
        .filter(|r| matches!(r, Reason::OptionKeyUnknownOnPin { .. }))
        .count();
    assert_eq!(count, 1, "expected exactly one reason, got {reasons:?}");
}
