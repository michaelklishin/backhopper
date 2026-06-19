// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `non_self_tracked` and the `self_projects` derivations cross-checked
//! against independent recomputation, plus the wire round-trip that
//! keeps `clearance_self_inferred` stable across serialization.

use std::collections::BTreeSet;

use proptest::prelude::*;

use backhopper_core::model::batch::{BatchPayload, BatchResult};
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    Diagnostics, PatchFacts, PinVerdict, SeriesVerdict, Verdict, non_self_tracked,
};

const PROJECTS: [&str; 3] = ["rabbit", "ra", "khepri"];

fn arb_pin() -> impl Strategy<Value = PinVerdict> {
    (prop::sample::select(PROJECTS.to_vec()), 0usize..6).prop_map(|(project, tracked)| {
        PinVerdict::new(
            Pin::new(
                ProjectName::new(project).unwrap(),
                TagName::new("v1.0.0").unwrap(),
            ),
            Verdict::Compatible,
        )
        .with_tracked_refs(tracked)
    })
}

fn arb_row() -> impl Strategy<Value = BatchResult> {
    (
        prop::sample::select(vec!['a', 'b', 'c']),
        prop::sample::select(vec!["s1", "s2"]),
        proptest::collection::vec(arb_pin(), 0..5),
    )
        .prop_map(|(c, series, pins)| BatchResult {
            commit: CommitSha::new(c.to_string().repeat(40)).unwrap(),
            series: SeriesName::new(series).unwrap(),
            verdict: SeriesVerdict::from_results(pins),
            diagnostics: Diagnostics::default(),
            patch_facts: PatchFacts::default(),
            touched_paths: Vec::new(),
            pr_commits: None,
            parent_count: None,
        })
}

fn arb_self_set() -> impl Strategy<Value = BTreeSet<ProjectName>> {
    proptest::collection::vec(prop::sample::select(PROJECTS.to_vec()), 0..3).prop_map(|names| {
        names
            .into_iter()
            .map(|n| ProjectName::new(n).unwrap())
            .collect()
    })
}

proptest! {
    #[test]
    fn non_self_tracked_matches_an_independent_sum(
        pins in proptest::collection::vec(arb_pin(), 0..6),
        self_projects in arb_self_set(),
    ) {
        let expected: u64 = pins
            .iter()
            .filter(|p| !self_projects.contains(&p.pin.project))
            .map(|p| p.tracked_refs as u64)
            .sum();
        let expected = u32::try_from(expected).unwrap_or(u32::MAX);
        prop_assert_eq!(non_self_tracked(&pins, &self_projects), expected);
    }

    #[test]
    fn a_wider_self_set_never_raises_the_tracked_sum(
        pins in proptest::collection::vec(arb_pin(), 0..6),
    ) {
        let narrow = BTreeSet::new();
        let wide = BTreeSet::from([ProjectName::new("rabbit").unwrap()]);
        prop_assert!(non_self_tracked(&pins, &wide) <= non_self_tracked(&pins, &narrow));
    }

    #[test]
    fn self_inferred_equals_explicit_clearance(
        rows in proptest::collection::vec(arb_row(), 0..6),
        self_projects in arb_self_set(),
    ) {
        let payload = BatchPayload {
            queried_against: Vec::new(),
            results: rows,
            self_projects: Some(self_projects.clone()),
        };
        prop_assert_eq!(
            payload.clearance_self_inferred(),
            Some(payload.clearance(&self_projects))
        );
    }

    #[test]
    fn clearance_survives_a_wire_round_trip(
        rows in proptest::collection::vec(arb_row(), 0..6),
        self_projects in arb_self_set(),
    ) {
        let payload = BatchPayload {
            queried_against: Vec::new(),
            results: rows,
            self_projects: Some(self_projects),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: BatchPayload = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.clearance_self_inferred(), payload.clearance_self_inferred());
    }
}
