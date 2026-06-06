// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    CallbackSig, FunArity, Module, Snapshot, SnapshotHeader, state,
};
use backhopper_core::model::verdict::{Reason, Verdict};
use time::OffsetDateTime;

fn header(tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("p").unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

fn behaviour_module(name: &str, callbacks: Vec<(&str, u8, &str)>) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    for (cb_name, arity, sig) in callbacks {
        m.callbacks.push(CallbackSig {
            name: FunctionName::new(cb_name).unwrap(),
            arity: Arity::new(arity),
            signature: sig.into(),
        });
    }
    m
}

fn impl_module(name: &str, behaviour: &str, exports: Vec<(&str, u8)>) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.behaviours.push(ModuleName::new(behaviour).unwrap());
    for (n, arity) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(n).unwrap(),
            arity: Arity::new(arity),
        });
    }
    m.exports.sort();
    m
}

fn canonical(tag: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header(tag), modules, Vec::new()).into_canonical()
}

fn evaluate(
    snapshot: Snapshot<state::Canonical>,
    source: Snapshot<state::Canonical>,
) -> Vec<Reason> {
    let pin = Pin::new(
        ProjectName::new("p").unwrap(),
        TagName::new("target").unwrap(),
    );
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &snapshot, Vec::new());
    let ctx = EvaluationContext::for_pin(pin, snapshot)
        .with_scope(scope.clone())
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let patch = Patch::parse("".as_bytes()).unwrap().analyze();
    let series = patch.evaluate_series(&[ctx]);
    match &series.verdict.results[0].verdict {
        Verdict::Compatible | Verdict::Inapplicable { .. } => Vec::new(),
        Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
            reasons.clone()
        }
    }
}

#[test]
fn signature_change_fires_on_implementer() {
    let pin_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> state()")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let src_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> {ok, state()}")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let reasons = evaluate(canonical("pin", pin_modules), canonical("src", src_modules));
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourCallbackSignatureChanged { .. })),
        "expected signature-changed reason, got {reasons:?}"
    );
}

#[test]
fn added_callback_fires_when_implementer_missing_export() {
    let pin_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> state()")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let src_modules = vec![
        behaviour_module(
            "bq",
            vec![
                ("init", 1, "init(A) -> state()"),
                ("recover", 2, "recover(A, B) -> ok"),
            ],
        ),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let reasons = evaluate(canonical("pin", pin_modules), canonical("src", src_modules));
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            Reason::BehaviourCallbackAdded { callback, .. } if callback.as_str() == "recover"
        )),
        "expected added-callback reason for recover/2, got {reasons:?}"
    );
}

#[test]
fn added_callback_silent_when_implementer_already_exports() {
    let pin_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> state()")]),
        impl_module("bq_impl", "bq", vec![("init", 1), ("recover", 2)]),
    ];
    let src_modules = vec![
        behaviour_module(
            "bq",
            vec![
                ("init", 1, "init(A) -> state()"),
                ("recover", 2, "recover(A, B) -> ok"),
            ],
        ),
        impl_module("bq_impl", "bq", vec![("init", 1), ("recover", 2)]),
    ];
    let reasons = evaluate(canonical("pin", pin_modules), canonical("src", src_modules));
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourCallbackAdded { .. })),
        "should not fire when implementer already has the export, got {reasons:?}"
    );
}

#[test]
fn removed_callback_fires_when_implementer_still_exports() {
    let pin_modules = vec![
        behaviour_module(
            "bq",
            vec![
                ("init", 1, "init(A) -> state()"),
                ("fold", 3, "fold(A, B, C) -> state()"),
            ],
        ),
        impl_module("bq_impl", "bq", vec![("init", 1), ("fold", 3)]),
    ];
    let src_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> state()")]),
        impl_module("bq_impl", "bq", vec![("init", 1), ("fold", 3)]),
    ];
    let reasons = evaluate(canonical("pin", pin_modules), canonical("src", src_modules));
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            Reason::BehaviourCallbackRemoved { callback, .. } if callback.as_str() == "fold"
        )),
        "expected removed-callback reason for fold/3, got {reasons:?}"
    );
}

#[test]
fn ast_equivalent_signatures_do_not_fire() {
    let pin_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> {ok, state()}")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let src_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> {ok ,    state()}")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let reasons = evaluate(canonical("pin", pin_modules), canonical("src", src_modules));
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourCallbackSignatureChanged { .. })),
        "AST-equivalent signatures should not fire, got {reasons:?}"
    );
}

#[test]
fn shape_differing_signatures_still_fire() {
    let pin_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> state()")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let src_modules = vec![
        behaviour_module("bq", vec![("init", 1, "init(A) -> {ok, state()}")]),
        impl_module("bq_impl", "bq", vec![("init", 1)]),
    ];
    let reasons = evaluate(canonical("pin", pin_modules), canonical("src", src_modules));
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourCallbackSignatureChanged { .. })),
        "real shape change should fire, got {reasons:?}"
    );
}

#[test]
fn no_conformance_check_without_source_snapshot() {
    let pin = canonical(
        "pin",
        vec![
            behaviour_module("bq", vec![("init", 1, "init(A) -> state()")]),
            impl_module("bq_impl", "bq", vec![("init", 1)]),
        ],
    );
    let patch = Patch::parse("".as_bytes()).unwrap().analyze();
    let pin_ref = Pin::new(
        ProjectName::new("p").unwrap(),
        TagName::new("target").unwrap(),
    );
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &pin, Vec::new());
    let ctx = EvaluationContext::for_pin(pin_ref, pin)
        .with_scope(scope)
        .with_files(EvaluationFiles::new());
    let series = patch.evaluate_series(&[ctx]);
    assert!(matches!(
        series.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}
