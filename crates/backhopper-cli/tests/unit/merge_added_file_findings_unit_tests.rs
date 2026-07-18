// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Verdict-merge semantics for the added-file findings: `Compatible` pins
//! promote to `RequiresAdaptation` when non-blocking reasons arrive,
//! existing reason vectors get appended to, `Inapplicable` rows stay
//! untouched, and per-suite diagnostic counters land on
//! `Diagnostics.missing_test_modules`.

use std::collections::BTreeMap;

use backhopper_cli::commands::target_repo::merge_added_file_findings_into_evaluation;
use backhopper_core::compat::added_file::AddedFileFindings;
use backhopper_core::model::names::{
    Arity, ModuleName, ProjectName, RelativePath, TagName, TypeName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, PatchFacts, PinVerdict, Reason, SeriesEvaluation,
    SeriesVerdict, Verdict,
};

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn mn(s: &str) -> ModuleName {
    ModuleName::new(s).unwrap()
}

fn pin(name: &str) -> Pin {
    Pin::new(
        ProjectName::new(name).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

fn eval_with(pins: Vec<PinVerdict>) -> SeriesEvaluation {
    SeriesEvaluation {
        verdict: SeriesVerdict::from_results(pins),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        apply: None,
        target_findings: None,
    }
}

fn missing_helper_reason() -> Reason {
    Reason::BehaviourModuleMissing {
        source_path: rp("deps/rabbit/src/x.erl"),
        behaviour: mn("custom_ct_hook"),
    }
}

#[test]
fn compatible_pin_promotes_to_requires_adaptation() {
    let mut eval = eval_with(vec![PinVerdict::new(pin("ra"), Verdict::Compatible)]);
    let findings = AddedFileFindings::new(vec![missing_helper_reason()], BTreeMap::new());
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::RequiresAdaptation { .. }
    ));
    assert_eq!(eval.verdict.summary.compatible, 0);
    assert_eq!(eval.verdict.summary.requires_adaptation, 1);
}

#[test]
fn requires_adaptation_pin_appends_reasons() {
    let existing = vec![Reason::MissingType {
        module: mn("ra_log"),
        name: TypeName::new("state").unwrap(),
        arity: Arity::new(0),
    }];
    let mut eval = eval_with(vec![PinVerdict::new(
        pin("ra"),
        Verdict::RequiresAdaptation { reasons: existing },
    )]);
    let findings = AddedFileFindings::new(vec![missing_helper_reason()], BTreeMap::new());
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    let Verdict::RequiresAdaptation { reasons } = &eval.verdict.results[0].verdict else {
        panic!("expected RequiresAdaptation");
    };
    assert_eq!(reasons.len(), 2);
}

#[test]
fn inapplicable_pin_is_left_untouched() {
    let mut eval = eval_with(vec![PinVerdict::new(
        pin("ra"),
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlyDocsTouched,
        },
    )]);
    let findings = AddedFileFindings::new(vec![missing_helper_reason()], BTreeMap::new());
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Inapplicable { .. }
    ));
}

#[test]
fn empty_findings_short_circuit_without_touching_evaluation() {
    let mut eval = eval_with(vec![PinVerdict::new(pin("ra"), Verdict::Compatible)]);
    let before = eval.clone();
    merge_added_file_findings_into_evaluation(AddedFileFindings::default(), &mut eval);
    assert_eq!(eval.verdict.summary, before.verdict.summary);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}

#[test]
fn missing_test_modules_merge_into_diagnostics() {
    let mut eval = eval_with(vec![PinVerdict::new(pin("ra"), Verdict::Compatible)]);
    let mut inner = BTreeMap::new();
    inner.insert(mn("amqp_utils"), 3);
    let mut outer = BTreeMap::new();
    outer.insert(rp("deps/rabbit/test/x_SUITE.erl"), inner);
    let findings = AddedFileFindings::new(Vec::new(), outer);
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    assert_eq!(eval.diagnostics.missing_test_modules.len(), 1);
    assert_eq!(
        eval.diagnostics
            .missing_test_modules
            .get(&rp("deps/rabbit/test/x_SUITE.erl"))
            .and_then(|m| m.get(&mn("amqp_utils")))
            .copied(),
        Some(3)
    );
    // No reasons → verdict stays Compatible.
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}

#[test]
fn missing_test_module_counts_accumulate_across_calls() {
    let mut eval = eval_with(vec![PinVerdict::new(pin("ra"), Verdict::Compatible)]);
    let suite = rp("deps/rabbit/test/x_SUITE.erl");
    let helper = mn("amqp_utils");
    eval.diagnostics
        .record_missing_test_module(suite.clone(), helper.clone());
    let mut inner = BTreeMap::new();
    inner.insert(helper.clone(), 2);
    let mut outer = BTreeMap::new();
    outer.insert(suite.clone(), inner);
    let findings = AddedFileFindings::new(Vec::new(), outer);
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    assert_eq!(
        eval.diagnostics
            .missing_test_modules
            .get(&suite)
            .and_then(|m| m.get(&helper))
            .copied(),
        Some(3)
    );
}

#[test]
fn empty_pin_list_swallows_reasons_without_panic() {
    let mut eval = eval_with(Vec::new());
    let findings = AddedFileFindings::new(vec![missing_helper_reason()], BTreeMap::new());
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    assert!(eval.verdict.results.is_empty());
    assert_eq!(eval.verdict.summary.requires_adaptation, 0);
}

#[test]
fn promotion_recounts_summary_across_multiple_pins() {
    let mut eval = eval_with(vec![
        PinVerdict::new(pin("ra"), Verdict::Compatible),
        PinVerdict::new(pin("khepri"), Verdict::Compatible),
        PinVerdict::new(
            pin("osiris"),
            Verdict::Inapplicable {
                reason: InapplicableReason::OnlyDocsTouched,
            },
        ),
    ]);
    let findings = AddedFileFindings::new(vec![missing_helper_reason()], BTreeMap::new());
    merge_added_file_findings_into_evaluation(findings, &mut eval);
    assert_eq!(eval.verdict.summary.compatible, 0);
    assert_eq!(eval.verdict.summary.requires_adaptation, 2);
    assert_eq!(eval.verdict.summary.inapplicable, 1);
}
