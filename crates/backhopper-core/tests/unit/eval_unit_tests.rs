// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::eval::{BuildOutcome, CorpusEntry, Ratio, evaluate_corpus};
use backhopper_core::model::evaluation::AggregateVerdict;
use backhopper_core::model::fingerprint::VerdictFingerprint;
use backhopper_core::model::resolver_coverage::{ResolverClass, ResolverCoverage};

fn entry(
    fp: &str,
    verdict: AggregateVerdict,
    outcome: BuildOutcome,
    coverage: Option<ResolverCoverage>,
) -> CorpusEntry {
    CorpusEntry {
        fingerprint: VerdictFingerprint::new(fp),
        verdict,
        outcome,
        coverage,
    }
}

fn compilation_failed(class: ResolverClass) -> BuildOutcome {
    BuildOutcome::CompilationFailed { class: Some(class) }
}

fn coverage_of(classes: &[ResolverClass]) -> ResolverCoverage {
    let names: Vec<String> = classes
        .iter()
        .map(|c| {
            serde_json::to_value(c)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    serde_json::from_value(serde_json::json!({ "checked": names })).unwrap()
}

#[test]
fn vacuous_trust_counts_clean_verdicts_that_built_clean() {
    let rows = [
        entry(
            "a",
            AggregateVerdict::Inapplicable,
            BuildOutcome::BuiltClean,
            None,
        ),
        entry(
            "b",
            AggregateVerdict::Compatible,
            BuildOutcome::BuiltClean,
            None,
        ),
        entry(
            "c",
            AggregateVerdict::Inapplicable,
            compilation_failed(ResolverClass::Macro),
            None,
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.vacuous_trust, Ratio { hits: 2, total: 3 });
}

#[test]
fn recall_counts_flagged_breaks_and_missed_records_the_rest() {
    let rows = [
        entry(
            "flagged",
            AggregateVerdict::Incompatible,
            compilation_failed(ResolverClass::Macro),
            Some(coverage_of(&[ResolverClass::Macro])),
        ),
        entry(
            "missed",
            AggregateVerdict::Inapplicable,
            compilation_failed(ResolverClass::LocalCall),
            Some(coverage_of(&[ResolverClass::LocalCall])),
        ),
        entry(
            "clean",
            AggregateVerdict::Compatible,
            BuildOutcome::BuiltClean,
            None,
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.recall, Ratio { hits: 1, total: 2 });
    assert_eq!(report.missed_breaks.len(), 1);
    let missed = &report.missed_breaks[0];
    assert_eq!(missed.fingerprint, VerdictFingerprint::new("missed"));
    assert_eq!(missed.break_class, Some(ResolverClass::LocalCall));
    assert_eq!(missed.is_resolver_bug, Some(true));
}

#[test]
fn precision_counts_real_breaks_among_flagged_rows() {
    let rows = [
        entry(
            "real",
            AggregateVerdict::Incompatible,
            compilation_failed(ResolverClass::Macro),
            None,
        ),
        entry(
            "false_alarm",
            AggregateVerdict::RequiresAdaptation,
            BuildOutcome::BuiltClean,
            None,
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.precision, Ratio { hits: 1, total: 2 });
}

#[test]
fn breaks_by_class_ranks_compile_failures_only() {
    let rows = [
        entry(
            "a",
            AggregateVerdict::Incompatible,
            compilation_failed(ResolverClass::Macro),
            None,
        ),
        entry(
            "b",
            AggregateVerdict::Inapplicable,
            compilation_failed(ResolverClass::Macro),
            None,
        ),
        entry(
            "c",
            AggregateVerdict::RequiresAdaptation,
            BuildOutcome::ApplyConflicted,
            None,
        ),
    ];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.rows, 3);
    assert!(report.breaks_by_class.contains(&(ResolverClass::Macro, 2)));
    // an apply conflict carries no resolver class, so it ranks nothing
    assert_eq!(report.breaks_by_class.len(), 1);
}

// The same break class routes bug-vs-gap by the row's recorded coverage, not the running binary's.
#[test]
fn bug_vs_gap_routes_by_the_rows_recorded_coverage() {
    let rows = [
        entry(
            "bug",
            AggregateVerdict::Inapplicable,
            compilation_failed(ResolverClass::Macro),
            Some(coverage_of(&[ResolverClass::Macro])),
        ),
        entry(
            "gap",
            AggregateVerdict::Inapplicable,
            compilation_failed(ResolverClass::Macro),
            Some(coverage_of(&[ResolverClass::Record])),
        ),
        entry(
            "unknown",
            AggregateVerdict::Inapplicable,
            compilation_failed(ResolverClass::Macro),
            None,
        ),
    ];
    let report = evaluate_corpus(&rows);
    let bug_of = |fp: &str| {
        report
            .missed_breaks
            .iter()
            .find(|m| m.fingerprint == VerdictFingerprint::new(fp))
            .unwrap()
            .is_resolver_bug
    };
    assert_eq!(bug_of("bug"), Some(true));
    assert_eq!(bug_of("gap"), Some(false));
    assert_eq!(bug_of("unknown"), None);
}

// An apply or test miss is a predictor miss on a different axis: no class, no bug flag.
#[test]
fn an_apply_miss_is_not_a_compile_coverage_gap() {
    let rows = [entry(
        "apply",
        AggregateVerdict::Inapplicable,
        BuildOutcome::ApplyConflicted,
        Some(ResolverCoverage::current()),
    )];
    let report = evaluate_corpus(&rows);
    assert_eq!(report.missed_breaks.len(), 1);
    let missed = &report.missed_breaks[0];
    assert_eq!(missed.break_class, None);
    assert_eq!(missed.is_resolver_bug, None);
    assert_eq!(missed.outcome, BuildOutcome::ApplyConflicted);
}

#[test]
fn an_empty_corpus_yields_zero_rates() {
    let report = evaluate_corpus(&[]);
    assert_eq!(report.rows, 0);
    assert_eq!(report.recall, Ratio { hits: 0, total: 0 });
    assert!(report.missed_breaks.is_empty());
}
