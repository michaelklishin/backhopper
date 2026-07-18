// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::model::batch::{BatchPayload, BatchQuery, BatchResult, PinPayload};
use backhopper_core::model::fingerprint::FINGERPRINT_VERSION;
use backhopper_core::model::names::{CommitSha, ProjectName, RelativePath, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::pr_commit::{PrCommit, PrCommitKind};
use backhopper_core::model::resolver_coverage::ResolverCoverage;
use backhopper_core::model::verdict::{Diagnostics, PatchFacts, SeriesSummary, SeriesVerdict};
use time::OffsetDateTime;

fn fixture_commit() -> CommitSha {
    CommitSha::new("a".repeat(40)).expect("forty hex chars")
}

fn empty_series_verdict() -> SeriesVerdict {
    SeriesVerdict {
        results: Vec::new(),
        summary: SeriesSummary::default(),
    }
}

#[test]
fn batch_result_round_trips_with_empty_collections() {
    let row = BatchResult {
        commit: fixture_commit(),
        series: SeriesName::new("rabbitmq-4.0").unwrap(),
        verdict: empty_series_verdict(),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let json = serde_json::to_string(&row).expect("serialise");
    let back: BatchResult = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, row);
}

#[test]
fn pr_commits_distinguishes_none_from_empty_some() {
    let mut row = BatchResult {
        commit: fixture_commit(),
        series: SeriesName::new("demo").unwrap(),
        verdict: empty_series_verdict(),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let json_none = serde_json::to_string(&row).expect("none serialises");
    row.pr_commits = Some(Vec::new());
    let json_empty = serde_json::to_string(&row).expect("empty some serialises");
    assert_ne!(
        json_none, json_empty,
        "None vs Some([]) must serialise differently"
    );
    assert!(json_none.contains("\"pr_commits\":null"));
    assert!(json_empty.contains("\"pr_commits\":[]"));
}

#[test]
fn pr_commits_some_empty_round_trips_through_serde() {
    let row = BatchResult {
        commit: fixture_commit(),
        series: SeriesName::new("demo").unwrap(),
        verdict: empty_series_verdict(),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: Some(Vec::new()),
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let json = serde_json::to_string(&row).unwrap();
    let back: BatchResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.pr_commits, Some(Vec::new()));
    assert_eq!(back, row);
}

#[test]
fn pr_commits_with_entries_round_trips() {
    let pc = PrCommit {
        sha: fixture_commit(),
        subject: "Resolve conflicts".into(),
        author_date: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        kind: PrCommitKind::ConflictResolution,
    };
    let row = BatchResult {
        commit: fixture_commit(),
        series: SeriesName::new("demo").unwrap(),
        verdict: empty_series_verdict(),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: Some(vec![pc.clone()]),
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let json = serde_json::to_string(&row).unwrap();
    let back: BatchResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.pr_commits, Some(vec![pc]));
}

#[test]
fn touched_paths_emit_in_diff_encounter_order() {
    let row = BatchResult {
        commit: fixture_commit(),
        series: SeriesName::new("demo").unwrap(),
        verdict: empty_series_verdict(),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: vec![
            RelativePath::new("z/last.erl").unwrap(),
            RelativePath::new("a/first.erl").unwrap(),
            RelativePath::new("m/mid.erl").unwrap(),
        ],
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let json = serde_json::to_value(&row).expect("serialise");
    let arr = json
        .get("touched_paths")
        .and_then(serde_json::Value::as_array)
        .expect("touched_paths array");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0].as_str(), Some("z/last.erl"));
    assert_eq!(arr[1].as_str(), Some("a/first.erl"));
    assert_eq!(arr[2].as_str(), Some("m/mid.erl"));
}

#[test]
fn pr_commit_kind_is_likely_bookkeeping_returns_false_only_for_substantive() {
    assert!(!PrCommitKind::Substantive.is_likely_bookkeeping());
    assert!(PrCommitKind::ConflictResolution.is_likely_bookkeeping());
    assert!(PrCommitKind::ReviewFeedback.is_likely_bookkeeping());
    assert!(PrCommitKind::Fixup.is_likely_bookkeeping());
    assert!(PrCommitKind::WipOrCleanup.is_likely_bookkeeping());
}

#[test]
fn pr_commit_kind_serialises_as_snake_case() {
    for (kind, expected) in [
        (PrCommitKind::Substantive, "\"substantive\""),
        (PrCommitKind::ConflictResolution, "\"conflict_resolution\""),
        (PrCommitKind::ReviewFeedback, "\"review_feedback\""),
        (PrCommitKind::Fixup, "\"fixup\""),
        (PrCommitKind::WipOrCleanup, "\"wip_or_cleanup\""),
    ] {
        assert_eq!(serde_json::to_string(&kind).unwrap(), expected);
    }
}

#[test]
fn pr_commit_carries_iso8601_timestamp() {
    let pc = PrCommit {
        sha: fixture_commit(),
        subject: "Resolve conflicts".to_owned(),
        author_date: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        kind: PrCommitKind::ConflictResolution,
    };
    let json = serde_json::to_value(&pc).unwrap();
    let stamp = json
        .get("author_date")
        .and_then(serde_json::Value::as_str)
        .expect("author_date is a string");
    assert!(stamp.starts_with("2023-"), "rfc3339 timestamp: {stamp}");
}

#[test]
fn batch_payload_full_round_trip_with_typed_pin_payload() {
    let payload = BatchPayload {
        queried_against: vec![BatchQuery {
            series: SeriesName::new("rabbitmq-4.0").unwrap(),
            pins: vec![PinPayload {
                project: ProjectName::new("ra").unwrap(),
                tag: TagName::from_str("v2.16.13").unwrap(),
            }],
        }],
        results: vec![],
        self_projects: Some(BTreeSet::from([ProjectName::new("rabbit").unwrap()])),
        resolver_coverage: None,
        fingerprint_version: None,
    };
    let json = serde_json::to_string(&payload).unwrap();
    let back: BatchPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(back, payload);
}

#[test]
fn resolver_coverage_and_fingerprint_version_round_trip() {
    let payload = BatchPayload {
        queried_against: vec![],
        results: vec![],
        self_projects: Some(BTreeSet::new()),
        resolver_coverage: Some(ResolverCoverage::current()),
        fingerprint_version: Some(FINGERPRINT_VERSION),
    };
    let json = serde_json::to_string(&payload).unwrap();
    let back: BatchPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(back, payload);
    assert_eq!(back.fingerprint_version, Some(FINGERPRINT_VERSION));
}

// An old producer omits both fields; they deserialize to None, not an
// empty coverage that would read as "checks nothing".
#[test]
fn a_pre_field_producer_parses_both_as_none() {
    let json = r#"{"queried_against":[],"results":[],"self_projects":[]}"#;
    let back: BatchPayload = serde_json::from_str(json).unwrap();
    assert_eq!(back.resolver_coverage, None);
    assert_eq!(back.fingerprint_version, None);
}

#[test]
fn pin_payload_projects_from_a_resolved_pin() {
    let pin = Pin {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::from_str("v2.16.13").unwrap(),
    };
    let payload = PinPayload::from(&pin);
    assert_eq!(payload.project, pin.project);
    assert_eq!(payload.tag, pin.tag);
}

// The owned projection moves every shared field and drops the batch-only
// ones, so a consumer needs no field-by-field repack.
#[test]
fn batch_result_converts_into_series_evaluation() {
    use backhopper_core::model::verdict::SeriesEvaluation;
    let path = RelativePath::new("deps/rabbit/src/rabbit.erl").unwrap();
    let row = BatchResult {
        commit: fixture_commit(),
        series: SeriesName::new("rabbitmq-4.0").unwrap(),
        verdict: empty_series_verdict(),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: vec![path.clone()],
        pr_commits: Some(Vec::new()),
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let eval: SeriesEvaluation = row.clone().into();
    assert_eq!(eval.verdict, row.verdict);
    assert_eq!(eval.touched_paths, vec![path]);
    assert_eq!(eval.pr_commits, Some(Vec::new()));
    assert_eq!(eval.apply, None);
    assert_eq!(eval.target_findings, None);
}
