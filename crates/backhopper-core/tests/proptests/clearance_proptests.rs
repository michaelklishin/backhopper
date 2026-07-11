// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `RoundClearance::from_results` cross-checked against an independent
//! recomputation over the same generated rows. The load-bearing
//! invariant is the discriminator: a round is clean exactly when it
//! carries no tracked-dep reference, no blocking verdict, and no
//! snapshot-missing bump.

use std::collections::{BTreeSet, HashSet};

use proptest::prelude::*;

use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::clearance::RoundClearance;
use backhopper_core::model::names::{CommitSha, DependencyName, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    BumpStatus, Diagnostics, InapplicableReason, PatchFacts, PinBump, PinVerdict, SeriesVerdict,
    Verdict, exit,
};

const SELF_PROJECT: &str = "rabbit";

fn arb_commit() -> impl Strategy<Value = CommitSha> {
    // A small alphabet so distinct commits collide and exercise dedup.
    prop::sample::select(vec!['a', 'b', 'c'])
        .prop_map(|c| CommitSha::new(c.to_string().repeat(40)).unwrap())
}

fn arb_verdict() -> impl Strategy<Value = Verdict> {
    prop_oneof![
        Just(Verdict::Compatible),
        Just(Verdict::RequiresAdaptation {
            reasons: Vec::new()
        }),
        Just(Verdict::Incompatible {
            reasons: Vec::new()
        }),
        prop::sample::select(vec![
            InapplicableReason::NoErlangSurfaceTouched,
            InapplicableReason::OnlyDocsTouched,
            InapplicableReason::OnlyMakefileTouched,
            InapplicableReason::OnlySchemaTouched,
        ])
        .prop_map(|reason| Verdict::Inapplicable { reason }),
    ]
}

fn arb_pin() -> impl Strategy<Value = PinVerdict> {
    (
        prop::sample::select(vec!["rabbit", "ra", "khepri"]),
        arb_verdict(),
        0usize..4,
    )
        .prop_map(|(project, verdict, tracked)| {
            PinVerdict::new(
                Pin::new(
                    ProjectName::new(project).unwrap(),
                    TagName::new("v1.0.0").unwrap(),
                ),
                verdict,
            )
            .with_tracked_refs(tracked)
        })
}

fn arb_bump() -> impl Strategy<Value = PinBump> {
    (
        prop::sample::select(vec!["ra", "khepri", "osiris"]),
        proptest::option::of(Just("hex 1.0.0".to_owned())),
        prop::sample::select(vec!["hex 2.0.0", "hex 3.0.0"]),
        prop_oneof![
            Just(None),
            Just(Some(BumpStatus::Untracked)),
            Just(Some(BumpStatus::SnapshotPresent)),
            Just(Some(BumpStatus::SnapshotMissing {
                note: "snapshots generate".to_owned()
            })),
        ],
    )
        .prop_map(|(dep, from, to, status)| PinBump {
            dep: DependencyName::new(dep).unwrap(),
            from,
            to: to.to_owned(),
            status,
        })
}

fn arb_row() -> impl Strategy<Value = BatchResult> {
    (
        arb_commit(),
        prop::sample::select(vec!["s1", "s2", "s3"]),
        proptest::collection::vec(arb_pin(), 0..4),
        proptest::collection::vec(arb_bump(), 0..3),
        proptest::collection::vec(prop::sample::select(vec!["a_SUITE", "b_SUITE"]), 0..3),
    )
        .prop_map(|(commit, series, pins, bumps, suites)| {
            let diagnostics = Diagnostics {
                pin_bumps: bumps,
                suggested_suites: suites.into_iter().map(str::to_owned).collect(),
                ..Diagnostics::default()
            };
            BatchResult {
                commit,
                series: SeriesName::new(series).unwrap(),
                verdict: SeriesVerdict::from_results(pins),
                diagnostics,
                patch_facts: PatchFacts::default(),
                touched_paths: Vec::new(),
                pr_commits: None,
                parent_count: None,
                verdict_fingerprint: None,
                apply: None,
            }
        })
}

proptest! {
    #[test]
    fn clearance_matches_an_independent_recomputation(
        rows in proptest::collection::vec(arb_row(), 0..8),
    ) {
        let mut self_projects = BTreeSet::new();
        self_projects.insert(ProjectName::new(SELF_PROJECT).unwrap());
        let clearance = RoundClearance::from_results(&rows, &self_projects);
        let facts = clearance.facts();

        let mut tracked: u64 = 0;
        let mut incompatible = 0u32;
        let mut requires = 0u32;
        let mut inapplicable_pins = 0u32;
        let mut commits = BTreeSet::new();
        let mut series = BTreeSet::new();
        let mut distinct_bumps = HashSet::new();
        let mut snapshot_missing = 0u32;
        for row in &rows {
            commits.insert(row.commit.clone());
            series.insert(row.series.clone());
            for pin in &row.verdict.results {
                if pin.pin.project.as_str() != SELF_PROJECT {
                    tracked += pin.tracked_refs as u64;
                }
                match &pin.verdict {
                    Verdict::Incompatible { .. } => incompatible += 1,
                    Verdict::RequiresAdaptation { .. } => requires += 1,
                    Verdict::Inapplicable { .. } => inapplicable_pins += 1,
                    Verdict::Compatible => {}
                }
            }
            for b in &row.diagnostics.pin_bumps {
                let fresh = distinct_bumps.insert((row.commit.clone(), b.dep.clone(), b.to.clone()));
                if fresh && matches!(b.status, Some(BumpStatus::SnapshotMissing { .. })) {
                    snapshot_missing += 1;
                }
            }
        }
        let tracked = u32::try_from(tracked).unwrap_or(u32::MAX);

        prop_assert_eq!(facts.rows, rows.len());
        prop_assert_eq!(facts.candidates, commits.len());
        prop_assert_eq!(facts.series, series.len());
        prop_assert_eq!(facts.tracked, tracked);
        prop_assert_eq!(facts.verdicts.incompatible, incompatible);
        prop_assert_eq!(facts.verdicts.requires_adaptation, requires);
        prop_assert_eq!(facts.reasons.total(), inapplicable_pins);
        prop_assert_eq!(facts.bumps.total, distinct_bumps.len() as u32);
        prop_assert_eq!(facts.bumps.snapshot_missing, snapshot_missing);

        let needs_attention = incompatible > 0 || requires > 0;
        let expected_exit = if needs_attention { exit::NEEDS_ATTENTION } else { exit::OK };
        prop_assert_eq!(facts.exit_code, expected_exit);

        let expect_clean = tracked == 0 && !needs_attention && snapshot_missing == 0;
        prop_assert_eq!(clearance.is_clean(), expect_clean);
    }
}
