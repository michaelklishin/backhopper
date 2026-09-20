// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::config::FamilyDefaults;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{CallbackSig, Module, Snapshot, state};
use backhopper_core::model::verdict::{DriftEvidence, Reason, Verdict};
use backhopper_test_support::{canonical_snapshot, snapshot_header};

fn behaviour_module(name: &str, callbacks: Vec<(&str, u8)>) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    for (cb_name, arity) in callbacks {
        m.callbacks.push(CallbackSig {
            name: FunctionName::new(cb_name).unwrap(),
            arity: Arity::new(arity),
            signature: format!("{cb_name}(A) -> ok"),
        });
    }
    m
}

fn canonical(tag: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("ra", tag), modules)
}

fn dep_behaviour_defaults() -> FamilyDefaults {
    FamilyDefaults {
        dep_behaviours: vec!["ra_machine".to_owned()],
        ..Default::default()
    }
}

fn evaluate(
    diff: &[u8],
    target: Snapshot<state::Canonical>,
    source: Option<Snapshot<state::Canonical>>,
) -> Vec<Reason> {
    let pin = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("target").unwrap(),
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, Vec::new());
    let mut ctx = EvaluationContext::for_pin(pin, target)
        .with_scope(scope)
        .with_files(EvaluationFiles::new())
        .with_family_defaults(dep_behaviour_defaults());
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

const ADD_SNAPSHOT_INSTALLED_4: &[u8] = b"\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,2 +1,4 @@
 -module(rabbit_fifo).
 -behaviour(ra_machine).
+-export([snapshot_installed/4]).
+snapshot_installed(Meta, State, Idx, Term) -> State.
";

#[test]
fn tier1_fires_for_live_indexes_with_source_side() {
    let target = canonical("2_17_x", vec![behaviour_module("ra_machine", vec![])]);
    let source = canonical(
        "3_1_x",
        vec![behaviour_module("ra_machine", vec![("live_indexes", 1)])],
    );
    let diff: &[u8] = b"\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,2 +1,4 @@
 -module(rabbit_fifo).
 -behaviour(ra_machine).
+-export([live_indexes/1]).
+live_indexes(State) -> [].
";
    let reasons = evaluate(diff, target, Some(source));
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            Reason::BehaviourCallbackUnknownOnPin {
                evidence: DriftEvidence::SourceSideSetDifference,
                ..
            }
        )),
        "expected tier-1 finding, got {reasons:?}"
    );
}

#[test]
fn tier1_silent_without_source_side_when_no_name_anchor() {
    let target = canonical("2_17_x", vec![behaviour_module("ra_machine", vec![])]);
    let diff: &[u8] = b"\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,2 +1,4 @@
 -module(rabbit_fifo).
 -behaviour(ra_machine).
+-export([live_indexes/1]).
+live_indexes(State) -> [].
";
    let reasons = evaluate(diff, target, None);
    assert!(
        reasons.is_empty(),
        "no name anchor and no source side: expected silence, got {reasons:?}"
    );
}

#[test]
fn tier2_name_anchored_fires_for_snapshot_installed_4_against_2_13() {
    let target = canonical(
        "2_13_x",
        vec![behaviour_module(
            "ra_machine",
            vec![("snapshot_installed", 2)],
        )],
    );
    let reasons = evaluate(ADD_SNAPSHOT_INSTALLED_4, target, None);
    let found = reasons.iter().find(|r| {
        matches!(
            r,
            Reason::BehaviourCallbackUnknownOnPin {
                evidence: DriftEvidence::NameAnchored,
                ..
            }
        )
    });
    let Some(Reason::BehaviourCallbackUnknownOnPin { pin_arities, .. }) = found else {
        panic!("expected tier-2 finding, got {reasons:?}");
    };
    assert_eq!(pin_arities, &[Arity::new(2)]);
}

#[test]
fn helper_function_does_not_fire() {
    let target = canonical(
        "2_13_x",
        vec![behaviour_module(
            "ra_machine",
            vec![("snapshot_installed", 2)],
        )],
    );
    let diff: &[u8] = b"\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,2 +1,4 @@
 -module(rabbit_fifo).
 -behaviour(ra_machine).
+-export([checkpoint_state/2]).
+checkpoint_state(A, B) -> ok.
";
    let reasons = evaluate(diff, target, None);
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn dual_behaviour_gen_server_init_suppressed() {
    let target = canonical(
        "2_13_x",
        vec![behaviour_module("ra_machine", vec![("init", 1)])],
    );
    let diff: &[u8] = b"\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,3 +1,5 @@
 -module(rabbit_fifo).
 -behaviour(ra_machine).
 -behaviour(gen_server).
+-export([init/1]).
+init(Args) -> {ok, Args}.
";
    let reasons = evaluate(diff, target, None);
    assert!(
        reasons.is_empty(),
        "gen_server init/1 should be suppressed by the OTP table, got {reasons:?}"
    );
}

#[test]
fn missing_on_pin_fires_for_added_implementer() {
    let target = canonical(
        "2_17_x",
        vec![behaviour_module("ra_machine", vec![("apply", 3)])],
    );
    let diff: &[u8] = b"\
diff --git a/new_impl.erl b/new_impl.erl
new file mode 100644
--- /dev/null
+++ b/new_impl.erl
@@ -0,0 +1,3 @@
+-module(new_impl).
+-behaviour(ra_machine).
+-export([init/1]).
";
    let reasons = evaluate(diff, target, None);
    assert!(
        reasons.iter().any(
            |r| matches!(r, Reason::BehaviourCallbackMissingOnPin { callback, arity, .. }
                if callback.as_str() == "apply" && arity.get() == 3)
        ),
        "expected missing-on-pin finding, got {reasons:?}"
    );
}

#[test]
fn existing_direction_unaffected() {
    // A patch that modifies the behaviour module itself still exercises
    // only `check_behaviour_conformance`, never the new C1 variants.
    let target = canonical(
        "2_17_x",
        vec![behaviour_module("ra_machine", vec![("apply", 3)])],
    );
    let diff: &[u8] = b"\
diff --git a/ra_machine.erl b/ra_machine.erl
--- a/ra_machine.erl
+++ b/ra_machine.erl
@@ -1,1 +1,2 @@
 -module(ra_machine).
+-export([helper/0]).
";
    let reasons = evaluate(diff, target, None);
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourCallbackUnknownOnPin { .. })),
        "modifying the behaviour module itself must not trigger C1, got {reasons:?}"
    );
}
