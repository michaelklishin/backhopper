// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use backhopper_core::model::eval::{
    ForecastEntry, ForecastReport, ForecastedConflict, ObservedApply, Ratio, evaluate_forecasts,
    paths_match,
};
use backhopper_core::model::names::{CommitSha, RelativePath};
use backhopper_core::model::verdict::ApplyConflictKind;

fn sha(hex_char: char) -> CommitSha {
    CommitSha::new(hex_char.to_string().repeat(40)).unwrap()
}

fn rel(path: &str) -> RelativePath {
    RelativePath::new(path).unwrap()
}

fn predicted(paths: &[&str]) -> Vec<ForecastedConflict> {
    paths
        .iter()
        .map(|p| ForecastedConflict {
            path: rel(p),
            kind: ApplyConflictKind::PreimageMissing,
        })
        .collect()
}

fn conflicted(paths: &[&str]) -> ObservedApply {
    ObservedApply::Conflicted {
        paths: paths.iter().map(|p| rel(p)).collect(),
    }
}

fn entry(
    sha_char: char,
    predicted: Vec<ForecastedConflict>,
    observed: ObservedApply,
) -> ForecastEntry {
    ForecastEntry {
        sha: sha(sha_char),
        predicted,
        observed,
        unconvertible_paths: 0,
    }
}

#[test]
fn an_empty_set_of_entries_yields_zero_rates() {
    let report = evaluate_forecasts(&[]);
    assert_eq!(report.entries, 0);
    assert_eq!(report.precision, Ratio { hits: 0, total: 0 });
    assert_eq!(report.recall, Ratio { hits: 0, total: 0 });
    assert_eq!(report.path_overlap, Ratio { hits: 0, total: 0 });
    assert!(report.false_negatives.is_empty());
}

#[test]
fn precision_and_recall_count_entries_not_paths() {
    let rows = [
        // true positive: predicted, conflicted
        entry(
            'a',
            predicted(&["deps/rabbit/src/rabbit_fifo.erl"]),
            conflicted(&["deps/rabbit/src/rabbit_fifo.erl"]),
        ),
        // false positive: predicted, applied clean
        entry('b', predicted(&["src/ra_server.erl"]), ObservedApply::Clean),
        // false negative: nothing predicted, conflicted
        entry('c', Vec::new(), conflicted(&["src/khepri_machine.erl"])),
        // true negative: nothing predicted, applied clean
        entry('d', Vec::new(), ObservedApply::Clean),
    ];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.entries, 4);
    assert_eq!(report.precision, Ratio { hits: 1, total: 2 });
    assert_eq!(report.recall, Ratio { hits: 1, total: 2 });
}

#[test]
fn out_of_band_entries_are_excluded_from_both_denominators_and_counted() {
    let rows = [
        entry(
            'a',
            predicted(&["src/ra_log.erl"]),
            ObservedApply::OutOfBand,
        ),
        entry('b', Vec::new(), ObservedApply::OutOfBand),
        entry(
            'c',
            predicted(&["src/ra_log.erl"]),
            conflicted(&["src/ra_log.erl"]),
        ),
    ];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.out_of_band, 2);
    assert_eq!(report.precision, Ratio { hits: 1, total: 1 });
    assert_eq!(report.recall, Ratio { hits: 1, total: 1 });
}

// the entry is a rate hit, yet the unpredicted path must reach the worklist
#[test]
fn a_true_positive_with_an_extra_observed_path_yields_a_partial_miss() {
    let rows = [entry(
        'a',
        predicted(&["deps/rabbit/src/rabbit_fifo.erl"]),
        conflicted(&[
            "deps/rabbit/src/rabbit_fifo.erl",
            "deps/rabbit/test/quorum_queue_SUITE.erl",
        ]),
    )];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.precision, Ratio { hits: 1, total: 1 });
    assert_eq!(report.recall, Ratio { hits: 1, total: 1 });
    assert_eq!(report.false_negatives.len(), 1);
    let miss = &report.false_negatives[0];
    assert_eq!(
        miss.missed_paths,
        BTreeSet::from([rel("deps/rabbit/test/quorum_queue_SUITE.erl")])
    );
    assert_eq!(miss.predicted.len(), 1);
}

#[test]
fn a_miss_with_an_empty_prediction_marks_an_unmodeled_axis() {
    let rows = [entry('a', Vec::new(), conflicted(&["src/osiris_log.erl"]))];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.false_negatives.len(), 1);
    assert!(report.false_negatives[0].predicted.is_empty());
}

// recall keys on the variant, never the path count
#[test]
fn a_conflict_with_no_normalized_paths_still_counts_toward_recall() {
    let rows = [ForecastEntry {
        sha: sha('a'),
        predicted: Vec::new(),
        observed: ObservedApply::Conflicted {
            paths: BTreeSet::new(),
        },
        unconvertible_paths: 2,
    }];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.recall, Ratio { hits: 0, total: 1 });
    assert_eq!(report.unconvertible_paths, 2);
    // no normalized paths, so no worklist row
    assert!(report.false_negatives.is_empty());
}

#[test]
fn unconvertible_paths_are_summed_across_entries() {
    let rows = [
        ForecastEntry {
            sha: sha('a'),
            predicted: Vec::new(),
            observed: ObservedApply::Clean,
            unconvertible_paths: 1,
        },
        ForecastEntry {
            sha: sha('b'),
            predicted: Vec::new(),
            observed: ObservedApply::OutOfBand,
            unconvertible_paths: 3,
        },
    ];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.unconvertible_paths, 4);
}

#[test]
fn path_overlap_counts_matched_predicted_paths_on_true_positives_only() {
    let rows = [
        entry(
            'a',
            predicted(&["src/rabbit_fifo.erl", "src/rabbit_fifo_client.erl"]),
            conflicted(&["deps/rabbit/src/rabbit_fifo.erl"]),
        ),
        // false positive: its predicted path never enters the denominator
        entry(
            'b',
            predicted(&["src/ra_server_proc.erl"]),
            ObservedApply::Clean,
        ),
    ];
    let report = evaluate_forecasts(&rows);
    assert_eq!(report.path_overlap, Ratio { hits: 1, total: 2 });
}

#[test]
fn the_matcher_accepts_equal_paths_and_boundary_suffixes_in_both_directions() {
    assert!(paths_match("src/ra_log.erl", "src/ra_log.erl"));
    assert!(paths_match("src/ra_log.erl", "deps/ra/src/ra_log.erl"));
    assert!(paths_match("deps/ra/src/ra_log.erl", "src/ra_log.erl"));
}

#[test]
fn the_matcher_rejects_a_suffix_that_crosses_a_segment_boundary() {
    assert!(!paths_match("a_log.erl", "src/ra_log.erl"));
    assert!(!paths_match("src/ra_log.erl", "a_log.erl"));
    assert!(!paths_match("src/ra_log.erl", "src/ra_log.erl.bak"));
}

#[test]
fn the_matcher_matches_a_bare_filename_against_any_nested_path() {
    assert!(paths_match("Makefile", "deps/rabbit/Makefile"));
    assert!(paths_match("deps/rabbitmq_management/Makefile", "Makefile"));
}

#[test]
fn forecast_entry_round_trips_through_json_with_the_documented_tags() {
    let row = entry(
        'a',
        predicted(&["src/rabbit_mgmt_wm_auth.erl"]),
        conflicted(&["src/rabbit_mgmt_wm_auth.erl"]),
    );
    let json = serde_json::to_value(&row).unwrap();
    assert_eq!(json["observed"]["kind"], "conflicted");
    assert_eq!(json["predicted"][0]["kind"], "preimage_missing");
    let back: ForecastEntry = serde_json::from_value(json).unwrap();
    assert_eq!(back, row);
}

#[test]
fn clean_and_out_of_band_serialize_as_bare_tag_objects() {
    let clean = serde_json::to_value(ObservedApply::Clean).unwrap();
    assert_eq!(clean, serde_json::json!({ "kind": "clean" }));
    let oob = serde_json::to_value(ObservedApply::OutOfBand).unwrap();
    assert_eq!(oob, serde_json::json!({ "kind": "out_of_band" }));
}

#[test]
fn a_report_round_trips_through_json() {
    let rows = [entry('a', Vec::new(), conflicted(&["src/khepri_tree.erl"]))];
    let report = evaluate_forecasts(&rows);
    let json = serde_json::to_value(&report).unwrap();
    let back: ForecastReport = serde_json::from_value(json).unwrap();
    assert_eq!(back, report);
}
