// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `SignatureChanged`: a source `-spec` that differs from the target's
//! fires; matching specs stay silent.

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName};
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SpecSig, state};
use backhopper_core::model::verdict::{Reason, Verdict};
use backhopper_test_support::pin;

use crate::evaluation_support::snapshot;

fn snapshot_with_module_specs(
    project: &str,
    module_name: &str,
    specs: Vec<(&str, u8, &str)>,
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new(module_name).unwrap());
    for (n, a, _) in &specs {
        m.exports.push(FunArity {
            name: FunctionName::new(*n).unwrap(),
            arity: Arity::new(*a),
        });
    }
    for (n, a, sig) in &specs {
        m.specs.push(SpecSig {
            name: FunctionName::new(*n).unwrap(),
            arity: Arity::new(*a),
            signature: (*sig).to_string(),
        });
    }
    snapshot(project, vec![m])
}

#[test]
fn signature_changed_fires_when_source_spec_differs_from_target() {
    let target =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), atom()) -> ok")]);
    let source =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), term()) -> ok")]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+build(R) -> ra:new(R, default).
";
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin("ra", "v1.0.0"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_sig_changed = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::SignatureChanged { module, function, arity, .. }
                if module.as_str() == "ra"
                    && function.as_str() == "new"
                    && *arity == Arity::new(2)
        )
    });
    assert!(
        has_sig_changed,
        "expected SignatureChanged reason, got {reasons:?}"
    );
}

#[test]
fn signature_changed_silent_when_specs_match() {
    let target =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), term()) -> ok")]);
    let source =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), term()) -> ok")]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+build(R) -> ra:new(R, default).
";
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin("ra", "v1.0.0"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}
