// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants of the forecast fold over arbitrary paired rows.

use std::collections::BTreeSet;

use proptest::prelude::*;

use backhopper_core::model::eval::{
    ForecastEntry, ForecastedConflict, ObservedApply, evaluate_forecasts, paths_match,
};
use backhopper_core::model::names::{CommitSha, RelativePath};
use backhopper_core::model::verdict::ApplyConflictKind;

fn sha(n: u16) -> CommitSha {
    CommitSha::new(format!("{n:040x}")).unwrap()
}

fn rel_path() -> impl Strategy<Value = RelativePath> {
    prop_oneof![
        Just("src/ra_log.erl"),
        Just("deps/ra/src/ra_log.erl"),
        Just("src/rabbit_fifo.erl"),
        Just("deps/rabbit/test/quorum_queue_SUITE.erl"),
        Just("Makefile"),
        Just("deps/rabbit/Makefile"),
    ]
    .prop_map(|p| RelativePath::new(p).unwrap())
}

fn kind() -> impl Strategy<Value = ApplyConflictKind> {
    prop_oneof![
        Just(ApplyConflictKind::PostimageCollision),
        Just(ApplyConflictKind::PreimageMissing),
        Just(ApplyConflictKind::FileAbsent),
    ]
}

fn forecasted() -> impl Strategy<Value = ForecastedConflict> {
    (rel_path(), kind()).prop_map(|(path, kind)| ForecastedConflict { path, kind })
}

fn observed() -> impl Strategy<Value = ObservedApply> {
    prop_oneof![
        Just(ObservedApply::Clean),
        proptest::collection::btree_set(rel_path(), 0..4)
            .prop_map(|paths| ObservedApply::Conflicted { paths }),
        Just(ObservedApply::OutOfBand),
    ]
}

fn entry_parts() -> impl Strategy<Value = (Vec<ForecastedConflict>, ObservedApply, usize)> {
    (
        proptest::collection::vec(forecasted(), 0..4),
        observed(),
        0usize..3,
    )
}

// SHAs are unique per index so per-entry properties can join on them
fn entries() -> impl Strategy<Value = Vec<ForecastEntry>> {
    proptest::collection::vec(entry_parts(), 0..12).prop_map(|parts| {
        parts
            .into_iter()
            .enumerate()
            .map(
                |(i, (predicted, observed, unconvertible_paths))| ForecastEntry {
                    sha: sha(u16::try_from(i).unwrap()),
                    predicted,
                    observed,
                    unconvertible_paths,
                },
            )
            .collect()
    })
}

proptest! {
    #[test]
    fn ratios_never_exceed_their_denominators(rows in entries()) {
        let report = evaluate_forecasts(&rows);
        prop_assert!(report.precision.hits <= report.precision.total);
        prop_assert!(report.recall.hits <= report.recall.total);
        prop_assert!(report.path_overlap.hits <= report.path_overlap.total);
    }

    // every non-out-of-band entry lands in the rate cells for its variant and prediction
    #[test]
    fn entry_partition_matches_the_variants(rows in entries()) {
        let report = evaluate_forecasts(&rows);
        let graded: Vec<_> = rows
            .iter()
            .filter(|e| !matches!(e.observed, ObservedApply::OutOfBand))
            .collect();
        let predicted = graded.iter().filter(|e| !e.predicted.is_empty()).count();
        let conflicted = graded
            .iter()
            .filter(|e| matches!(e.observed, ObservedApply::Conflicted { .. }))
            .count();
        prop_assert_eq!(report.precision.total, predicted);
        prop_assert_eq!(report.recall.total, conflicted);
        prop_assert_eq!(report.precision.hits, report.recall.hits);
        prop_assert_eq!(report.out_of_band, rows.len() - graded.len());
        prop_assert_eq!(report.entries, rows.len());
    }

    // every observed path of a conflicted entry is matched by a prediction or sits in exactly one miss row
    #[test]
    fn path_partition_covers_every_observed_conflicting_path(rows in entries()) {
        let report = evaluate_forecasts(&rows);
        for e in &rows {
            let ObservedApply::Conflicted { paths } = &e.observed else {
                continue;
            };
            let miss_rows: Vec<_> = report
                .false_negatives
                .iter()
                .filter(|m| m.sha == e.sha)
                .collect();
            for path in paths {
                let matched = e
                    .predicted
                    .iter()
                    .any(|p| paths_match(p.path.as_str(), path.as_str()));
                let missed = miss_rows
                    .iter()
                    .filter(|m| m.missed_paths.contains(path))
                    .count();
                prop_assert_eq!(missed, usize::from(!matched));
            }
        }
    }

    #[test]
    fn unconvertible_paths_sum_over_all_entries(rows in entries()) {
        let report = evaluate_forecasts(&rows);
        let expected: usize = rows.iter().map(|e| e.unconvertible_paths).sum();
        prop_assert_eq!(report.unconvertible_paths, expected);
    }

    #[test]
    fn entries_round_trip_through_json(rows in entries()) {
        let json = serde_json::to_string(&rows).unwrap();
        let back: Vec<ForecastEntry> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, rows);
    }

    #[test]
    fn the_matcher_is_symmetric_and_reflexive(a in rel_path(), b in rel_path()) {
        prop_assert!(paths_match(a.as_str(), a.as_str()));
        prop_assert_eq!(paths_match(a.as_str(), b.as_str()), paths_match(b.as_str(), a.as_str()));
    }
}

// duplicate SHAs across entries must not merge miss rows
#[test]
fn miss_rows_stay_per_entry_even_with_a_shared_sha() {
    let path = RelativePath::new("src/ra_log.erl").unwrap();
    let row = ForecastEntry {
        sha: sha(1),
        predicted: Vec::new(),
        observed: ObservedApply::Conflicted {
            paths: BTreeSet::from([path]),
        },
        unconvertible_paths: 0,
    };
    let report = evaluate_forecasts(&[row.clone(), row]);
    assert_eq!(report.false_negatives.len(), 2);
}
