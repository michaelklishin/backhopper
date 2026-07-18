// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `ApplyForecast`: the per-path severity fold, the accessors the
//! clearance and renderers read, the wire encoding, and the
//! forecast-and-reasons union in `BatchResult::predicted_conflicts`.

use std::path::PathBuf;

use backhopper_core::model::apply::{ApplyForecast, PathApplyOutcome, UnassessedReason};
use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::names::{CommitSha, ProjectName, RelativePath, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    ApplyConflictKind, Diagnostics, InapplicableReason, PatchFacts, PinVerdict, Reason,
    SeriesVerdict, Verdict,
};

fn rel(path: &str) -> RelativePath {
    RelativePath::new(path.to_owned()).unwrap()
}

fn conflict(kind: ApplyConflictKind) -> PathApplyOutcome {
    PathApplyOutcome::Conflict { kind }
}

#[test]
fn recording_a_conflict_over_a_clean_outcome_keeps_the_conflict() {
    let mut forecast = ApplyForecast::default();
    let path = rel("deps/rabbit/src/rabbit_amqqueue.erl");
    forecast.record(path.clone(), PathApplyOutcome::CleanExact);
    forecast.record(path.clone(), conflict(ApplyConflictKind::PreimageMissing));
    forecast.record(
        path.clone(),
        PathApplyOutcome::CleanDrifted { line_delta: 3 },
    );
    assert_eq!(
        forecast.paths.get(&path),
        Some(&conflict(ApplyConflictKind::PreimageMissing))
    );
}

#[test]
fn two_drifts_keep_the_largest_magnitude_delta() {
    let mut forecast = ApplyForecast::default();
    let path = rel("deps/rabbit/src/rabbit_reader.erl");
    forecast.record(
        path.clone(),
        PathApplyOutcome::CleanDrifted { line_delta: 4 },
    );
    forecast.record(
        path.clone(),
        PathApplyOutcome::CleanDrifted { line_delta: -11 },
    );
    assert_eq!(
        forecast.paths.get(&path),
        Some(&PathApplyOutcome::CleanDrifted { line_delta: -11 })
    );
}

#[test]
fn two_conflicts_keep_the_worse_kind() {
    let mut forecast = ApplyForecast::default();
    let path = rel("deps/rabbit/src/rabbit_channel.erl");
    forecast.record(
        path.clone(),
        conflict(ApplyConflictKind::PostimageCollision),
    );
    forecast.record(path.clone(), conflict(ApplyConflictKind::FileAbsent));
    forecast.record(path.clone(), conflict(ApplyConflictKind::PreimageMissing));
    assert_eq!(
        forecast.paths.get(&path),
        Some(&conflict(ApplyConflictKind::FileAbsent))
    );
}

#[test]
fn unassessed_ranks_above_clean_and_below_conflict() {
    let unassessed = PathApplyOutcome::Unassessed {
        reason: UnassessedReason::BinaryFile,
    };
    assert!(unassessed.severity() > PathApplyOutcome::CleanDrifted { line_delta: 9 }.severity());
    assert!(unassessed.severity() < conflict(ApplyConflictKind::PostimageCollision).severity());
    assert!(!unassessed.is_conflict());
}

#[test]
fn has_conflict_and_worst_and_summary_read_a_mixed_forecast() {
    let mut forecast = ApplyForecast::default();
    forecast.record(
        rel("deps/rabbit/src/rabbit_quorum_queue.erl"),
        PathApplyOutcome::CleanExact,
    );
    forecast.record(
        rel("deps/rabbit/src/rabbit_fifo.erl"),
        PathApplyOutcome::CleanDrifted { line_delta: 2 },
    );
    forecast.record(
        rel("deps/rabbit/priv/logo.png"),
        PathApplyOutcome::Unassessed {
            reason: UnassessedReason::BinaryFile,
        },
    );
    forecast.record(
        rel("deps/rabbit/test/quorum_queue_SUITE.erl"),
        conflict(ApplyConflictKind::PreimageMissing),
    );
    assert!(forecast.has_conflict());
    let summary = forecast.summary();
    assert_eq!(summary.clean, 1);
    assert_eq!(summary.drifted, 1);
    assert_eq!(summary.unassessed, 1);
    assert_eq!(summary.conflicted, 1);
    let conflicts: Vec<_> = forecast.conflicts().collect();
    assert_eq!(
        conflicts,
        vec![(
            &rel("deps/rabbit/test/quorum_queue_SUITE.erl"),
            ApplyConflictKind::PreimageMissing
        )]
    );
}

#[test]
fn clean_forecast_reports_no_conflict() {
    let mut forecast = ApplyForecast::default();
    forecast.record(
        rel("deps/rabbit/src/rabbit_amqqueue.erl"),
        PathApplyOutcome::CleanExact,
    );
    assert!(!forecast.has_conflict());
    assert_eq!(forecast.conflicts().count(), 0);
}

#[test]
fn wire_tags_are_snake_case_and_round_trip() {
    let mut forecast = ApplyForecast::default();
    forecast.record(
        rel("deps/rabbit/test/unit_amqp_reader_SUITE.erl"),
        conflict(ApplyConflictKind::PreimageMissing),
    );
    forecast.record(
        rel("deps/rabbit/src/rabbit_reader.erl"),
        PathApplyOutcome::CleanDrifted { line_delta: -4 },
    );
    forecast.record(
        rel("deps/rabbit/priv/logo.png"),
        PathApplyOutcome::Unassessed {
            reason: UnassessedReason::BinaryFile,
        },
    );
    let json = serde_json::to_value(&forecast).unwrap();
    assert_eq!(
        json["paths"]["deps/rabbit/test/unit_amqp_reader_SUITE.erl"]["outcome"],
        "conflict"
    );
    assert_eq!(
        json["paths"]["deps/rabbit/test/unit_amqp_reader_SUITE.erl"]["kind"],
        "preimage_missing"
    );
    assert_eq!(
        json["paths"]["deps/rabbit/src/rabbit_reader.erl"]["outcome"],
        "clean_drifted"
    );
    assert_eq!(
        json["paths"]["deps/rabbit/priv/logo.png"]["reason"],
        "binary_file"
    );
    let back: ApplyForecast = serde_json::from_value(json).unwrap();
    assert_eq!(back, forecast);
}

#[test]
fn conflict_kind_labels_are_stable() {
    assert_eq!(
        ApplyConflictKind::PreimageMissing.as_str(),
        "preimage_missing"
    );
    assert_eq!(ApplyConflictKind::FileAbsent.as_str(), "file_absent");
    assert_eq!(
        ApplyConflictKind::PostimageCollision.as_str(),
        "postimage_collision"
    );
}

fn inapplicable_row(apply: Option<ApplyForecast>) -> BatchResult {
    let pin = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v2.16.5").unwrap(),
    );
    let verdict = Verdict::Inapplicable {
        reason: InapplicableReason::OnlyTestFixturesTouched,
    };
    BatchResult {
        commit: CommitSha::new("a".repeat(40)).unwrap(),
        series: SeriesName::new("v4.0.x").unwrap(),
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(pin, verdict)]),
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

// The operator case: an all-inapplicable row keeps its conflicts
// visible through predicted_conflicts even though pin reasons are
// structurally empty for inapplicable verdicts.
#[test]
fn predicted_conflicts_reads_the_forecast_on_an_inapplicable_row() {
    let mut forecast = ApplyForecast::default();
    forecast.record(
        rel("deps/rabbit/test/quorum_queue_SUITE.erl"),
        conflict(ApplyConflictKind::PreimageMissing),
    );
    let row = inapplicable_row(Some(forecast));
    let predicted = row.predicted_conflicts();
    assert_eq!(predicted.len(), 1);
    assert_eq!(
        predicted[0].path,
        PathBuf::from("deps/rabbit/test/quorum_queue_SUITE.erl")
    );
    assert_eq!(predicted[0].kind, ApplyConflictKind::PreimageMissing);
}

#[test]
fn predicted_conflicts_unions_reasons_and_forecast_at_max_severity() {
    let pin = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v2.16.5").unwrap(),
    );
    let reasons = vec![Reason::PreimageMissing {
        path: PathBuf::from("deps/rabbit/src/rabbit_fifo.erl"),
        hunk_index: 0,
        preimage_excerpt: "apply(Meta, Cmd, State)".to_owned(),
    }];
    let mut forecast = ApplyForecast::default();
    forecast.record(
        rel("deps/rabbit/src/rabbit_fifo.erl"),
        conflict(ApplyConflictKind::FileAbsent),
    );
    forecast.record(
        rel("deps/rabbit/test/ra_SUITE.erl"),
        conflict(ApplyConflictKind::PreimageMissing),
    );
    let mut row = inapplicable_row(Some(forecast));
    row.verdict = SeriesVerdict::from_results(vec![PinVerdict::new(
        pin,
        Verdict::RequiresAdaptation { reasons },
    )]);
    let predicted = row.predicted_conflicts();
    assert_eq!(predicted.len(), 2);
    // the same path appears in both sources; the worse kind wins
    assert_eq!(predicted[0].kind, ApplyConflictKind::FileAbsent);
    assert_eq!(predicted[1].kind, ApplyConflictKind::PreimageMissing);
}

#[test]
fn predicted_conflicts_without_forecast_still_reads_reasons() {
    let pin = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v2.16.5").unwrap(),
    );
    let reasons = vec![Reason::PostimageCollision {
        path: PathBuf::from("deps/rabbit/src/rabbit_fifo.erl"),
        hunk_index: 1,
    }];
    let mut row = inapplicable_row(None);
    row.verdict = SeriesVerdict::from_results(vec![PinVerdict::new(
        pin,
        Verdict::RequiresAdaptation { reasons },
    )]);
    let predicted = row.predicted_conflicts();
    assert_eq!(predicted.len(), 1);
    assert_eq!(predicted[0].kind, ApplyConflictKind::PostimageCollision);
}
