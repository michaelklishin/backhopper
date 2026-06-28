// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The measurement types a consumer needs reach it through the driver
//! façade: a dropped re-export fails this build, not the consumer's.

use backhopper_driver::types::{
    AggregateVerdict, ApplyConflictKind, BuildOutcome, CorpusEntry, EvalReport,
    FINGERPRINT_VERSION, MissedBreak, PredictedConflict, Ratio, ResolverClass, ResolverCoverage,
    SymbolKind, VerdictFingerprint,
};

// Naming every type and calling a method through the façade is the guard:
// it stops compiling the day a re-export is dropped.
#[test]
fn measurement_types_reach_the_consumer_through_the_facade() {
    let coverage = ResolverCoverage::current();
    assert!(coverage.is_checked(ResolverClass::Macro));
    assert_eq!(
        ResolverClass::of_symbol_kind(&SymbolKind::Macro {
            name: "MAGIC".to_owned()
        }),
        ResolverClass::Macro
    );

    let entry = CorpusEntry {
        fingerprint: VerdictFingerprint::new("fp"),
        verdict: AggregateVerdict::Compatible,
        outcome: BuildOutcome::CompilationFailed {
            class: Some(ResolverClass::Macro),
        },
        coverage: Some(coverage),
    };
    assert_eq!(entry.outcome.break_class(), Some(ResolverClass::Macro));
    assert_eq!(FINGERPRINT_VERSION, FINGERPRINT_VERSION);

    fn _names(_: EvalReport, _: MissedBreak, _: PredictedConflict, _: Ratio, _: ApplyConflictKind) {
    }
}
