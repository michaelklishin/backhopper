// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants of the corpus fold over arbitrary paired rows.

use proptest::prelude::*;

use backhopper_core::model::eval::{BuildOutcome, CorpusEntry, evaluate_corpus};
use backhopper_core::model::evaluation::AggregateVerdict;
use backhopper_core::model::fingerprint::VerdictFingerprint;
use backhopper_core::model::resolver_coverage::{ResolverClass, ResolverCoverage};

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

fn class() -> impl Strategy<Value = ResolverClass> {
    (0..ResolverClass::ALL.len()).prop_map(|i| ResolverClass::ALL[i])
}

fn verdict() -> impl Strategy<Value = AggregateVerdict> {
    prop_oneof![
        Just(AggregateVerdict::Compatible),
        Just(AggregateVerdict::RequiresAdaptation),
        Just(AggregateVerdict::Incompatible),
        Just(AggregateVerdict::Inapplicable),
        Just(AggregateVerdict::Empty),
    ]
}

fn outcome() -> impl Strategy<Value = BuildOutcome> {
    prop_oneof![
        Just(BuildOutcome::BuiltClean),
        proptest::option::of(class()).prop_map(|class| BuildOutcome::CompilationFailed { class }),
        Just(BuildOutcome::ApplyConflicted),
        Just(BuildOutcome::TestRegressed),
    ]
}

fn coverage() -> impl Strategy<Value = Option<ResolverCoverage>> {
    proptest::option::of(
        proptest::collection::vec(class(), 0..ResolverClass::ALL.len())
            .prop_map(|v| coverage_of(&v)),
    )
}

fn entry() -> impl Strategy<Value = CorpusEntry> {
    (any::<u16>(), verdict(), outcome(), coverage()).prop_map(|(n, verdict, outcome, coverage)| {
        CorpusEntry {
            fingerprint: VerdictFingerprint::new(format!("fp{n}")),
            verdict,
            outcome,
            coverage,
        }
    })
}

proptest! {
    #[test]
    fn rates_never_exceed_their_totals(rows in proptest::collection::vec(entry(), 0..24)) {
        let r = evaluate_corpus(&rows);
        prop_assert!(r.vacuous_trust.hits <= r.vacuous_trust.total);
        prop_assert!(r.recall.hits <= r.recall.total);
        prop_assert!(r.precision.hits <= r.precision.total);
        prop_assert_eq!(r.rows, rows.len());
    }

    #[test]
    fn recall_total_is_the_broken_row_count(rows in proptest::collection::vec(entry(), 0..24)) {
        let broken = rows.iter().filter(|e| e.outcome.is_break()).count();
        prop_assert_eq!(evaluate_corpus(&rows).recall.total, broken);
    }

    // Missed breaks are exactly the unflagged breaks; a bug-vs-gap verdict needs both a class and row coverage.
    #[test]
    fn missed_breaks_are_the_unflagged_breaks(rows in proptest::collection::vec(entry(), 0..24)) {
        let expected = rows
            .iter()
            .filter(|e| e.outcome.is_break() && !e.verdict.flagged())
            .count();
        let r = evaluate_corpus(&rows);
        prop_assert_eq!(r.missed_breaks.len(), expected);
        for m in &r.missed_breaks {
            if m.is_resolver_bug.is_some() {
                prop_assert!(m.break_class.is_some());
            }
            prop_assert_eq!(m.break_class, m.outcome.break_class());
        }
    }
}
