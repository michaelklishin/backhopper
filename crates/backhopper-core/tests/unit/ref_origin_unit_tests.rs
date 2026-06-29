// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Reference provenance: added lines drive verdicts, context lines
//! drive diagnostics, and a kind-level dedup keeps the stronger
//! origin.

use std::str::FromStr;

use backhopper_core::SymbolKind;
use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{FunctionName, Mfa, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::symbol::{RefOrigin, SymbolRef};
use backhopper_core::model::verdict::{SeriesEvaluation, Verdict};
use backhopper_test_support::{canonical_snapshot, module_with, snapshot_header};

fn dep_snapshot(exports: &[(&str, u8)]) -> Snapshot<state::Canonical> {
    canonical_snapshot(
        snapshot_header("dep", "v1.0.0"),
        vec![module_with("dep_mod", exports)],
    )
}

fn patch_with(added: &str, context: &str) -> Vec<u8> {
    format!(
        "diff --git a/src/user.erl b/src/user.erl\n--- a/src/user.erl\n+++ b/src/user.erl\n@@ -1,2 +1,3 @@\n {context}\n+{added}\n other() -> ok.\n"
    )
    .into_bytes()
}

fn evaluate(bytes: &[u8], snapshot: Snapshot<state::Canonical>) -> SeriesEvaluation {
    let pin = Pin::new(
        ProjectName::new("dep").unwrap(),
        TagName::new("v1.0.0").unwrap(),
    );
    let scope = PinScope::from_snapshot(pin.project.clone(), &snapshot, Vec::new());
    let ctx = EvaluationContext::new(pin, snapshot, scope);
    Patch::parse(bytes)
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx])
}

#[test]
fn added_line_reference_drives_the_verdict() {
    let bytes = patch_with("go(X) -> dep_mod:gone(X).", "head() -> ok.");
    let eval = evaluate(&bytes, dep_snapshot(&[("present", 1)]));
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Incompatible { .. }
    ));
}

// The same unresolved call on a context line is a pre-existing target
// fact: diagnostics, never a reason.
#[test]
fn context_line_reference_lands_in_diagnostics_not_the_verdict() {
    let bytes = patch_with("go(X) -> X.", "old(X) -> dep_mod:gone(X).");
    let eval = evaluate(&bytes, dep_snapshot(&[("present", 1)]));
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Compatible
    ));
    let missing = &eval.diagnostics.context_refs_missing;
    assert_eq!(
        missing.get(&ModuleName::new("dep_mod").unwrap()).copied(),
        Some(1)
    );
}

#[test]
fn resolved_context_reference_is_silent() {
    let bytes = patch_with("go(X) -> X.", "old(X) -> dep_mod:present(X).");
    let eval = evaluate(&bytes, dep_snapshot(&[("present", 1)]));
    assert!(eval.diagnostics.context_refs_missing.is_empty());
}

// The same MFA on an added and a context line dedups to Added, so the
// verdict still fires.
#[test]
fn added_origin_wins_the_dedup() {
    let bytes = patch_with("go(X) -> dep_mod:gone(X).", "old(X) -> dep_mod:gone(X).");
    let eval = evaluate(&bytes, dep_snapshot(&[("present", 1)]));
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Incompatible { .. }
    ));
}

#[test]
fn tracked_refs_count_added_origin_references_of_every_kind() {
    let bytes = patch_with(
        "go(X) -> dep_mod:present(X), fun dep_mod:present/1.",
        "old(X) -> dep_mod:present(X).",
    );
    let eval = evaluate(&bytes, dep_snapshot(&[("present", 1)]));
    // The direct call and the fun ref dedup to one Function kind.
    assert_eq!(eval.verdict.results[0].tracked_refs, 1);
}

#[test]
fn origin_default_is_added_and_stays_off_the_wire() {
    let r = SymbolRef::function(Mfa::from_str("m:f/1").unwrap());
    let json = serde_json::to_value(&r).unwrap();
    assert!(json.get("origin").is_none());
    let back: SymbolRef = serde_json::from_value(json).unwrap();
    assert_eq!(back.origin, RefOrigin::Added);
    let ctx = serde_json::to_value(r.with_origin(RefOrigin::Context)).unwrap();
    assert_eq!(ctx.get("origin").unwrap(), "context");
}

#[test]
fn context_only_dep_patch_reads_as_vacuous_not_verified() {
    let bytes = patch_with("go(X) -> X.", "old(X) -> dep_mod:present(X).");
    let eval = evaluate(&bytes, dep_snapshot(&[("present", 1)]));
    assert_eq!(eval.verdict.results[0].tracked_refs, 0);
}

// A call whose argument list wraps across two added lines. The old
// per-line scanner saw `dep_mod:gone(X,` unterminated and emitted a
// `FunctionAnyArity`, which resolved against `gone/1` and passed. The
// joined scan resolves it at arity 2.
fn wrapped_call_patch() -> Vec<u8> {
    "diff --git a/src/user.erl b/src/user.erl\n\
     --- a/src/user.erl\n\
     +++ b/src/user.erl\n\
     @@ -1,1 +1,3 @@\n\
     +go(X) -> dep_mod:gone(X,\n\
     +                      Y).\n\
      other() -> ok.\n"
        .as_bytes()
        .to_vec()
}

// The verified false negative: a wrapped call to `gone/2` against a
// snapshot exporting only `gone/1` is no longer accepted.
#[test]
fn a_wrapped_added_call_resolves_at_exact_arity() {
    let eval = evaluate(&wrapped_call_patch(), dep_snapshot(&[("gone", 1)]));
    assert!(
        !matches!(eval.verdict.results[0].verdict, Verdict::Compatible),
        "wrapped gone/2 against gone/1 must not pass: {:?}",
        eval.verdict.results[0].verdict
    );
}

// The same wrapped call resolves clean when the exact arity exists, so
// the recovery does not introduce a false positive.
#[test]
fn a_wrapped_added_call_at_a_present_arity_is_clean() {
    let eval = evaluate(&wrapped_call_patch(), dep_snapshot(&[("gone", 2)]));
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}

#[test]
fn symbol_kind_match_is_exhaustive_for_any_arity() {
    // FunctionAnyArity participates in scope and tally paths; this
    // pins the variant's wire tag.
    let m = ModuleName::new("m").unwrap();
    let f = FunctionName::new("f").unwrap();
    let s = SymbolRef::function_any_arity(m, f);
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json.get("kind").unwrap(), "function_any_arity");
    assert!(matches!(s.kind, SymbolKind::FunctionAnyArity { .. }));
}
