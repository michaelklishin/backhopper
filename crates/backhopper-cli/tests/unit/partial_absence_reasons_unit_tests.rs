// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `partial_absence_reasons` and its merge: a patch straddling
//! present and absent target paths reports each absent path as a
//! non-blocking `TargetPathAbsent`, while an all-absent patch stays
//! the whole-patch `PathsMissingOnTarget` case (empty here).

use std::path::PathBuf;

use backhopper_cli::commands::target_repo::{
    TouchedPathSummary, merge_reasons_into_evaluation, partial_absence_reasons,
};
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, PatchFacts, PinVerdict, Reason, SeriesEvaluation,
    SeriesVerdict, Verdict,
};

fn summary(missing: &[&str], on_target: usize) -> TouchedPathSummary {
    TouchedPathSummary {
        renames: Vec::new(),
        missing: missing.iter().map(PathBuf::from).collect(),
        on_target,
    }
}

fn absent_paths(reasons: &[Reason]) -> Vec<&str> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::TargetPathAbsent { path } => Some(path.as_str()),
            _ => None,
        })
        .collect()
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
    }
}

#[test]
fn partial_absence_names_each_absent_path() {
    let reasons = partial_absence_reasons(&summary(
        &["deps/rabbit/src/rabbit_stream_super_stream_mgmt.erl"],
        2,
    ));
    assert_eq!(
        absent_paths(&reasons),
        ["deps/rabbit/src/rabbit_stream_super_stream_mgmt.erl"]
    );
}

// All paths absent is the whole-patch `PathsMissingOnTarget`
// inapplicable case, handled in `merge_into_series_verdict`.
#[test]
fn all_absent_yields_no_partial_reasons() {
    let reasons = partial_absence_reasons(&summary(&["a/x.erl", "b/y.erl"], 0));
    assert!(reasons.is_empty());
}

#[test]
fn no_missing_yields_no_reasons() {
    let reasons = partial_absence_reasons(&summary(&[], 3));
    assert!(reasons.is_empty());
}

#[test]
fn compatible_pin_promotes_to_requires_adaptation() {
    let mut eval = eval_with(vec![PinVerdict::new(pin("ra"), Verdict::Compatible)]);
    let reasons = partial_absence_reasons(&summary(&["a/x.erl"], 1));
    merge_reasons_into_evaluation(reasons, &mut eval);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::RequiresAdaptation { .. }
    ));
    assert_eq!(eval.verdict.summary.requires_adaptation, 1);
}

#[test]
fn requires_adaptation_pin_appends_the_absence() {
    let existing = vec![Reason::FileAbsent {
        path: PathBuf::from("a/other.erl"),
    }];
    let mut eval = eval_with(vec![PinVerdict::new(
        pin("ra"),
        Verdict::RequiresAdaptation { reasons: existing },
    )]);
    let reasons = partial_absence_reasons(&summary(&["a/x.erl"], 1));
    merge_reasons_into_evaluation(reasons, &mut eval);
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
    let reasons = partial_absence_reasons(&summary(&["a/x.erl"], 1));
    merge_reasons_into_evaluation(reasons, &mut eval);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Inapplicable { .. }
    ));
}
