// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-MIT and LICENSE-APACHE for details.

//! `ApplyForecast` invariants: the per-path fold is order-independent
//! and monotone, a forecast conflict always forces `Findings` and a
//! non-zero exit, and the wire encoding round-trips.

use std::collections::BTreeSet;

use proptest::prelude::*;

use backhopper_core::model::apply::{ApplyForecast, PathApplyOutcome, UnassessedReason};
use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::clearance::RoundClearance;
use backhopper_core::model::names::{CommitSha, ProjectName, RelativePath, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    ApplyConflictKind, Diagnostics, InapplicableReason, PatchFacts, PinVerdict, SeriesVerdict,
    Verdict, exit,
};

fn arb_outcome() -> impl Strategy<Value = PathApplyOutcome> {
    prop_oneof![
        Just(PathApplyOutcome::CleanExact),
        (-50i64..50).prop_map(|line_delta| PathApplyOutcome::CleanDrifted { line_delta }),
        prop::sample::select(vec![
            UnassessedReason::BinaryFile,
            UnassessedReason::TargetNotText,
        ])
        .prop_map(|reason| PathApplyOutcome::Unassessed { reason }),
        prop::sample::select(vec![
            ApplyConflictKind::PostimageCollision,
            ApplyConflictKind::PreimageMissing,
            ApplyConflictKind::FileAbsent,
        ])
        .prop_map(|kind| PathApplyOutcome::Conflict { kind }),
    ]
}

fn arb_path() -> impl Strategy<Value = RelativePath> {
    // A small path alphabet so re-recordings of the same path occur.
    prop::sample::select(vec![
        "deps/rabbit/src/rabbit_fifo.erl",
        "deps/rabbit/test/quorum_queue_SUITE.erl",
        "deps/rabbitmq_management/src/rabbit_mgmt_wm_auth.erl",
    ])
    .prop_map(|p| RelativePath::new(p.to_owned()).unwrap())
}

fn forecast_from(observations: &[(RelativePath, PathApplyOutcome)]) -> ApplyForecast {
    let mut forecast = ApplyForecast::default();
    for (path, outcome) in observations {
        forecast.record(path.clone(), outcome.clone());
    }
    forecast
}

fn inapplicable_row(apply: Option<ApplyForecast>) -> BatchResult {
    let pin = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v2.16.5").unwrap(),
    );
    BatchResult {
        commit: CommitSha::new("a".repeat(40)).unwrap(),
        series: SeriesName::new("v4.0.x").unwrap(),
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(
            pin,
            Verdict::Inapplicable {
                reason: InapplicableReason::OnlyTestFixturesTouched,
            },
        )]),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply,
        target_findings: None,
    }
}

proptest! {
    // The fold is a commutative, associative max: observation order never changes the forecast.
    #[test]
    fn recording_order_never_changes_the_forecast(
        observations in proptest::collection::vec((arb_path(), arb_outcome()), 0..12),
        seed in any::<u64>(),
    ) {
        let folded = forecast_from(&observations);
        let mut shuffled = observations.clone();
        // A deterministic shuffle from the seed keeps the case replayable.
        let mut state = seed;
        for i in (1..shuffled.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            shuffled.swap(i, (state as usize) % (i + 1));
        }
        prop_assert_eq!(folded, forecast_from(&shuffled));
    }

    // Recording one more observation never lowers a path's severity.
    #[test]
    fn recording_is_monotone_in_severity(
        observations in proptest::collection::vec((arb_path(), arb_outcome()), 1..12),
        extra in arb_outcome(),
    ) {
        let mut forecast = forecast_from(&observations);
        let path = observations[0].0.clone();
        let before = forecast.paths.get(&path).unwrap().severity();
        forecast.record(path.clone(), extra);
        prop_assert!(forecast.paths.get(&path).unwrap().severity() >= before);
    }

    // A predicted conflict is never reported as a clean or zero-domain clearance, whatever the verdicts.
    #[test]
    fn a_forecast_conflict_always_forces_findings(
        observations in proptest::collection::vec((arb_path(), arb_outcome()), 0..12),
    ) {
        let forecast = forecast_from(&observations);
        let conflicted = forecast.has_conflict();
        let rows = vec![inapplicable_row(Some(forecast))];
        let clearance = RoundClearance::from_results(&rows, &BTreeSet::new());
        if conflicted {
            prop_assert!(matches!(clearance, RoundClearance::Findings(_)));
            prop_assert_eq!(clearance.facts().exit_code, exit::NEEDS_ATTENTION);
        } else {
            prop_assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
            prop_assert_eq!(clearance.facts().exit_code, exit::OK);
        }
    }

    // Adding a conflict to a round never lowers its exit code.
    #[test]
    fn adding_a_conflict_never_lowers_the_exit_code(
        observations in proptest::collection::vec((arb_path(), arb_outcome()), 0..8),
    ) {
        let base = vec![inapplicable_row(Some(forecast_from(&observations)))];
        let base_exit = RoundClearance::from_results(&base, &BTreeSet::new())
            .facts()
            .exit_code;
        let mut worse_forecast = forecast_from(&observations);
        worse_forecast.record(
            RelativePath::new("Makefile".to_owned()).unwrap(),
            PathApplyOutcome::Conflict {
                kind: ApplyConflictKind::PreimageMissing,
            },
        );
        let worse = vec![inapplicable_row(Some(worse_forecast))];
        let worse_exit = RoundClearance::from_results(&worse, &BTreeSet::new())
            .facts()
            .exit_code;
        prop_assert!(worse_exit >= base_exit);
        prop_assert_eq!(worse_exit, exit::NEEDS_ATTENTION);
    }

    #[test]
    fn forecast_serde_round_trips(
        observations in proptest::collection::vec((arb_path(), arb_outcome()), 0..12),
    ) {
        let forecast = forecast_from(&observations);
        let json = serde_json::to_string(&forecast).unwrap();
        let back: ApplyForecast = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, forecast);
    }
}
