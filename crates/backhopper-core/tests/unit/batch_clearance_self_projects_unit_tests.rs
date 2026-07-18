// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `BatchPayload::self_projects` and the `clearance` and
//! `clearance_self_inferred` derivations: the `Some([])`-versus-`None`
//! wire distinction, self-pin exclusion, and the round-global self set.

use std::collections::BTreeSet;

use backhopper_core::model::batch::{BatchPayload, BatchResult};
use backhopper_core::model::clearance::RoundClearance;
use backhopper_core::model::names::{CommitSha, DependencyName, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    BumpStatus, Diagnostics, PatchFacts, PinBump, PinVerdict, SeriesVerdict, Verdict,
};

fn proj(name: &str) -> ProjectName {
    ProjectName::new(name).unwrap()
}

fn commit(c: char) -> CommitSha {
    CommitSha::new(c.to_string().repeat(40)).unwrap()
}

fn tracked_pin(project: &str, tracked: usize) -> PinVerdict {
    PinVerdict::new(
        Pin::new(proj(project), TagName::new("v1.0.0").unwrap()),
        Verdict::Compatible,
    )
    .with_tracked_refs(tracked)
}

fn row(c: char, series: &str, pins: Vec<PinVerdict>) -> BatchResult {
    row_with_bumps(c, series, pins, Vec::new())
}

fn row_with_bumps(
    c: char,
    series: &str,
    pins: Vec<PinVerdict>,
    pin_bumps: Vec<PinBump>,
) -> BatchResult {
    BatchResult {
        commit: commit(c),
        series: SeriesName::new(series).unwrap(),
        verdict: SeriesVerdict::from_results(pins),
        diagnostics: Diagnostics {
            pin_bumps,
            ..Diagnostics::default()
        },
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    }
}

fn payload(
    results: Vec<BatchResult>,
    self_projects: Option<BTreeSet<ProjectName>>,
) -> BatchPayload {
    BatchPayload {
        queried_against: Vec::new(),
        results,
        self_projects,
        resolver_coverage: None,
        fingerprint_version: None,
    }
}

fn self_set(names: &[&str]) -> BTreeSet<ProjectName> {
    names.iter().map(|n| proj(n)).collect()
}

#[test]
fn clearance_delegates_to_from_results() {
    let results = vec![row('a', "v4.1.x", vec![tracked_pin("ra", 2)])];
    let set = self_set(&["rabbit"]);
    let payload = payload(results.clone(), Some(set.clone()));
    assert_eq!(
        payload.clearance(&set),
        RoundClearance::from_results(&results, &set)
    );
}

#[test]
fn self_inferred_uses_the_wire_set() {
    let results = vec![row(
        'a',
        "v4.1.x",
        vec![tracked_pin("rabbit", 9), tracked_pin("ra", 4)],
    )];
    let set = self_set(&["rabbit"]);
    let payload = payload(results, Some(set.clone()));
    let inferred = payload.clearance_self_inferred().expect("self set present");
    assert_eq!(inferred, payload.clearance(&set));
    // The self pin's nine refs do not count; only ra's four do.
    assert_eq!(inferred.facts().tracked, 4);
}

#[test]
fn self_inferred_is_none_when_the_producer_omitted_the_field() {
    let results = vec![row('a', "v4.1.x", vec![tracked_pin("ra", 4)])];
    let payload = payload(results, None);
    assert!(payload.clearance_self_inferred().is_none());
}

#[test]
fn empty_self_set_still_yields_a_clearance() {
    let results = vec![row('a', "v4.1.x", vec![tracked_pin("ra", 4)])];
    let payload = payload(results, Some(BTreeSet::new()));
    let inferred = payload
        .clearance_self_inferred()
        .expect("empty set is present");
    assert_eq!(inferred.facts().tracked, 4);
}

#[test]
fn multiple_self_projects_are_all_excluded() {
    let results = vec![row(
        'a',
        "v4.1.x",
        vec![
            tracked_pin("rabbit", 9),
            tracked_pin("rabbit_common", 3),
            tracked_pin("ra", 5),
        ],
    )];
    let payload = payload(results, Some(self_set(&["rabbit", "rabbit_common"])));
    assert_eq!(
        payload.clearance_self_inferred().unwrap().facts().tracked,
        5
    );
}

#[test]
fn present_empty_self_set_round_trips_distinct_from_absent() {
    let results = vec![row('a', "v4.1.x", vec![tracked_pin("ra", 4)])];

    // A current producer always emits the field, even empty.
    let present = payload(results.clone(), Some(BTreeSet::new()));
    let json = serde_json::to_string(&present).unwrap();
    assert!(
        json.contains("self_projects"),
        "empty self set must serialize, not be skipped: {json}"
    );
    let back: BatchPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(back.self_projects, Some(BTreeSet::new()));
    assert!(back.clearance_self_inferred().is_some());

    // An old producer omits the key; serde default leaves it None.
    let absent: BatchPayload =
        serde_json::from_value(serde_json::json!({ "queried_against": [], "results": [] }))
            .unwrap();
    assert_eq!(absent.self_projects, None);
    assert!(absent.clearance_self_inferred().is_none());
}

#[test]
fn snapshot_missing_bump_is_findings_even_with_zero_tracked() {
    let bump = PinBump {
        dep: DependencyName::new("ra").unwrap(),
        from: Some("hex 2.16.0".to_owned()),
        to: "hex 2.17.0".to_owned(),
        status: Some(BumpStatus::SnapshotMissing {
            note: "snapshots generate".to_owned(),
        }),
    };
    let results = vec![row_with_bumps('a', "v4.1.x", Vec::new(), vec![bump])];
    let payload = payload(results, Some(BTreeSet::new()));
    let clearance = payload.clearance_self_inferred().unwrap();
    assert_eq!(clearance.facts().tracked, 0);
    assert!(
        !clearance.is_clean(),
        "a snapshot-missing bump is a finding regardless of the tracked count"
    );
}

#[test]
fn self_set_is_round_global_across_series() {
    // The round-global total sums non-self refs across both series.
    let results = vec![
        row(
            'a',
            "v4.1.x",
            vec![tracked_pin("rabbit", 9), tracked_pin("ra", 4)],
        ),
        row(
            'a',
            "v4.0.x",
            vec![tracked_pin("rabbit", 9), tracked_pin("khepri", 6)],
        ),
    ];
    let payload = payload(results, Some(self_set(&["rabbit"])));
    assert_eq!(
        payload.clearance_self_inferred().unwrap().facts().tracked,
        10
    );
}
