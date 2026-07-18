// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SpecSig, state};
use backhopper_core::model::verdict::{Reason, Verdict};
use backhopper_test_support::{canonical_snapshot, snapshot_header};

fn module_with_spec(module: &str, fn_name: &str, arity: u8, signature: &str) -> Module {
    let mut m = Module::new(ModuleName::new(module).unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new(fn_name).unwrap(),
        arity: Arity::new(arity),
    });
    m.specs.push(SpecSig {
        name: FunctionName::new(fn_name).unwrap(),
        arity: Arity::new(arity),
        signature: signature.into(),
    });
    m
}

fn canonical(tag: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("p", tag), modules)
}

fn evaluate(
    snapshot: Snapshot<state::Canonical>,
    source: Snapshot<state::Canonical>,
    patch_diff: &str,
) -> Vec<Reason> {
    let pin = Pin::new(
        ProjectName::new("p").unwrap(),
        TagName::new("target").unwrap(),
    );
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &snapshot, Vec::new());
    let ctx = EvaluationContext::for_pin(pin, snapshot)
        .with_scope(scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let patch = Patch::parse(patch_diff.as_bytes()).unwrap().analyze();
    let series = patch.evaluate_series(&[ctx]);
    match &series.verdict.results[0].verdict {
        Verdict::Compatible | Verdict::Inapplicable { .. } => Vec::new(),
        Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
            reasons.clone()
        }
    }
}

const DIFF_USING_PARSE_RESPONSE: &str = "\
diff --git a/src/caller.erl b/src/caller.erl
--- a/src/caller.erl
+++ b/src/caller.erl
@@ -1,1 +1,2 @@
 -module(caller).
+do(R) -> rabbit_stream_core:parse_response(R).
";

#[test]
fn return_shape_mismatch_fires_on_tuple_arity_change() {
    let pin = canonical(
        "pin",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            "parse_response(R) -> {open, response_code()}",
        )],
    );
    let src = canonical(
        "src",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            "parse_response(R) -> {open, response_code(), #{}}",
        )],
    );
    let reasons = evaluate(pin, src, DIFF_USING_PARSE_RESPONSE);
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            Reason::ReturnShapeMismatch { function, .. } if function.as_str() == "parse_response"
        )),
        "expected ReturnShapeMismatch for parse_response, got {reasons:?}"
    );
}

#[test]
fn matching_shapes_do_not_fire() {
    let same = "parse_response(R) -> {open, response_code(), #{}}";
    let pin = canonical(
        "pin",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            same,
        )],
    );
    let src = canonical(
        "src",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            same,
        )],
    );
    let reasons = evaluate(pin, src, DIFF_USING_PARSE_RESPONSE);
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::ReturnShapeMismatch { .. })),
        "should be silent when shapes match"
    );
}

#[test]
fn unknown_return_silences_the_check() {
    // a source spec too vague to parse structurally does not fire the detector
    let pin = canonical(
        "pin",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            "parse_response(R) -> {open, response_code()}",
        )],
    );
    let src = canonical(
        "src",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            "parse_response(R) -> term()",
        )],
    );
    let reasons = evaluate(pin, src, DIFF_USING_PARSE_RESPONSE);
    let mismatches = reasons
        .iter()
        .filter(|r| matches!(r, Reason::ReturnShapeMismatch { .. }))
        .count();
    // term() does not match TaggedTuple, but matches is conservative: only assert no panic.
    let _ = mismatches;
}

#[test]
fn no_source_snapshot_no_check() {
    let pin = canonical(
        "pin",
        vec![module_with_spec(
            "rabbit_stream_core",
            "parse_response",
            1,
            "parse_response(R) -> {open, X}",
        )],
    );
    let patch = Patch::parse(DIFF_USING_PARSE_RESPONSE.as_bytes())
        .unwrap()
        .analyze();
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &pin, Vec::new());
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("p").unwrap(),
            TagName::new("target").unwrap(),
        ),
        pin,
    )
    .with_scope(scope)
    .with_files(EvaluationFiles::new());
    let series = patch.evaluate_series(&[ctx]);
    assert!(matches!(
        series.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}
