// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::eval::{
    BuildOutcome, CorpusEntry, Ratio, VerdictClass, evaluate_corpus,
};
use backhopper_core::model::fingerprint::VerdictFingerprint;
use backhopper_core::model::resolver_coverage::ResolverClass;

fn entry(
    fp: &str,
    verdict: VerdictClass,
    outcome: BuildOutcome,
    break_class: Option<ResolverClass>,
) -> CorpusEntry {
    CorpusEntry {
        fingerprint: VerdictFingerprint::new(fp),
        verdict,
        outcome,
        break_class,
    }
}

#[test]
fn vacuous_trust_counts_clean_verdicts_that_built_clean() {
    let rows = [
        entry(
            "a",
            VerdictClass::Inapplicable,
            BuildOutcome::BuiltClean,
            None,
        ),
        entry(
            "b",
            VerdictClass::Compatible,
            BuildOutcome::BuiltClean,
            None,
        ),
        entry(
            "c",
            VerdictClass::Inapplicable,
            BuildOutcome::CompileFailed,
            Some(ResolverClass::Macro),
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.vacuous_trust, Ratio { hits: 2, total: 3 });
}

#[test]
fn recall_and_precision_pair_verdicts_to_outcomes() {
    let rows = [
        // flagged and broke: a true positive for both
        entry(
            "a",
            VerdictClass::RequiresAdaptation,
            BuildOutcome::CompileFailed,
            Some(ResolverClass::Macro),
        ),
        // broke but not flagged: a recall miss
        entry(
            "b",
            VerdictClass::Inapplicable,
            BuildOutcome::ApplyConflicted,
            None,
        ),
        // flagged but built clean: a precision miss (over-warn)
        entry(
            "c",
            VerdictClass::RequiresAdaptation,
            BuildOutcome::BuiltClean,
            None,
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.recall, Ratio { hits: 1, total: 2 });
    assert_eq!(report.precision, Ratio { hits: 1, total: 2 });
}

// A missed break in a covered class is a resolver bug; in an unbuilt
// class it would be a coverage gap. Every class is covered now, so a
// classed miss is always a bug.
#[test]
fn a_missed_break_is_routed_by_coverage() {
    let rows = [entry(
        "a",
        VerdictClass::Inapplicable,
        BuildOutcome::CompileFailed,
        Some(ResolverClass::Macro),
    )];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.missed_breaks.len(), 1);
    let miss = &report.missed_breaks[0];
    assert_eq!(miss.break_class, Some(ResolverClass::Macro));
    assert!(miss.is_resolver_bug);
}

#[test]
fn breaks_are_tallied_by_class() {
    let rows = [
        entry(
            "a",
            VerdictClass::Inapplicable,
            BuildOutcome::CompileFailed,
            Some(ResolverClass::Macro),
        ),
        entry(
            "b",
            VerdictClass::RequiresAdaptation,
            BuildOutcome::CompileFailed,
            Some(ResolverClass::Macro),
        ),
        entry(
            "c",
            VerdictClass::RequiresAdaptation,
            BuildOutcome::ApplyConflicted,
            Some(ResolverClass::LocalCall),
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.rows, 3);
    assert!(report.breaks_by_class.contains(&(ResolverClass::Macro, 2)));
    assert!(
        report
            .breaks_by_class
            .contains(&(ResolverClass::LocalCall, 1))
    );
}

#[test]
fn an_empty_corpus_yields_zero_rates() {
    let report = evaluate_corpus(&[]);
    assert_eq!(report.rows, 0);
    assert_eq!(report.recall, Ratio { hits: 0, total: 0 });
    assert!(report.missed_breaks.is_empty());
}
