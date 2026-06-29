// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `ClauseMismatch`: a call whose argument shape satisfies no clause
//! head fires; variable args, empty clause heads, and an already-fired
//! `SignatureChanged` all suppress it.

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName};
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SpecSig, state};
use backhopper_core::model::verdict::Reason;
use backhopper_test_support::{module_with, pin};

use crate::evaluation_support::{make_context, snapshot};

fn snapshot_with_clauses(
    project: &str,
    module_name: &str,
    function_name: &str,
    arity: u8,
    clauses: Vec<Vec<ArgShape>>,
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new(module_name).unwrap());
    let fa = FunArity {
        name: FunctionName::new(function_name).unwrap(),
        arity: Arity::new(arity),
    };
    m.exports.push(fa.clone());
    m.clause_heads.insert(fa, clauses);
    snapshot(project, vec![m])
}

#[test]
fn clause_mismatch_fires_when_args_dont_satisfy_clause_head() {
    let snap = snapshot_with_clauses(
        "ra",
        "ra",
        "mode",
        1,
        vec![
            vec![ArgShape::Atom {
                name: "start".into(),
            }],
            vec![ArgShape::Atom {
                name: "stop".into(),
            }],
        ],
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx = EvaluationContext::new(pin("ra", "v1.0.0"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_clause_mismatch = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::ClauseMismatch { module, function, arity, .. }
                if module.as_str() == "ra"
                    && function.as_str() == "mode"
                    && *arity == Arity::new(1)
        )
    });
    assert!(
        has_clause_mismatch,
        "expected ClauseMismatch reason, got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_silent_when_clauses_empty() {
    let context = make_context("ra", vec![module_with("ra", &[("mode", 1)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::ClauseMismatch { .. })),
        "no ClauseMismatch when clause_heads is empty, got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_fires_when_one_of_many_calls_fails_match() {
    let snap = snapshot_with_clauses(
        "ra",
        "ra",
        "mode",
        1,
        vec![vec![ArgShape::Atom {
            name: "start".into(),
        }]],
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx = EvaluationContext::new(pin("ra", "v1.0.0"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+ok_call() -> ra:mode(start).
+bad_call() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_clause_mismatch = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::ClauseMismatch { call_args, .. }
                if call_args.iter().any(|a| matches!(a, ArgShape::Atom { name } if name == "restart"))
        )
    });
    assert!(
        has_clause_mismatch,
        "ClauseMismatch must surface the failing call (restart), got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_silent_when_call_arg_is_variable() {
    let snap = snapshot_with_clauses(
        "ra",
        "ra",
        "mode",
        1,
        vec![vec![ArgShape::Atom {
            name: "start".into(),
        }]],
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx = EvaluationContext::new(pin("ra", "v1.0.0"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run(Cmd) -> ra:mode(Cmd).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::ClauseMismatch { .. })),
        "variable call args are unknown ground; must not flag, got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_suppressed_when_signature_changed_already_fired() {
    let make_module = |spec: &str| {
        let mut m = Module::new(ModuleName::new("ra").unwrap());
        let fa = FunArity {
            name: FunctionName::new("mode").unwrap(),
            arity: Arity::new(1),
        };
        m.exports.push(fa.clone());
        m.specs.push(SpecSig {
            name: FunctionName::new("mode").unwrap(),
            arity: Arity::new(1),
            signature: spec.into(),
        });
        m.clause_heads.insert(
            fa,
            vec![vec![ArgShape::Atom {
                name: "start".into(),
            }]],
        );
        m
    };
    let target = snapshot("ra", vec![make_module("mode(atom()) -> ok")]);
    let source = snapshot("ra", vec![make_module("mode(term()) -> ok")]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin("ra", "v1.0.0"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_signature_changed = reasons
        .iter()
        .any(|r| matches!(r, Reason::SignatureChanged { .. }));
    let has_clause_mismatch = reasons
        .iter()
        .any(|r| matches!(r, Reason::ClauseMismatch { .. }));
    assert!(
        has_signature_changed,
        "SignatureChanged must fire first, got {reasons:?}"
    );
    assert!(
        !has_clause_mismatch,
        "ClauseMismatch must be suppressed when SignatureChanged is already present, got {reasons:?}"
    );
}
