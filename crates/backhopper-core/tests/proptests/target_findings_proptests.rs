// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-MIT and LICENSE-APACHE for details.

//! Invariants of the clearance fold over rows carrying symbol-axis
//! target findings.

use std::collections::BTreeSet;
use std::str::FromStr;

use proptest::prelude::*;

use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::clearance::RoundClearance;
use backhopper_core::model::findings::TargetFindings;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, RelativePath, SeriesName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, IndirectCallForm, PatchFacts, PinVerdict, Reason,
    SeriesVerdict, Verdict,
};

fn sha(n: usize) -> CommitSha {
    CommitSha::new(format!("{n:040x}")).unwrap()
}

fn reason(function: &str, arity: u8) -> Reason {
    Reason::IndirectCallUndefinedOnTarget {
        source_path: RelativePath::new("deps/rabbit/test/maintenance_mode_SUITE.erl").unwrap(),
        module: ModuleName::from_str("rabbit_queue_type").unwrap(),
        function: FunctionName::from_str(function).unwrap(),
        arity: Arity::new(arity),
        via: IndirectCallForm::MeckExpect,
        line: 100,
    }
}

fn reasons() -> impl Strategy<Value = Vec<Reason>> {
    proptest::collection::vec(
        prop_oneof![
            Just(reason("drain", 1)),
            Just(reason("revive", 0)),
            Just(reason("status", 0)),
            Just(Reason::TargetPathAbsent {
                path: RelativePath::new("deps/rabbit/priv/rabbit.schema").unwrap(),
            }),
        ],
        0..4,
    )
}

fn findings() -> impl Strategy<Value = Option<TargetFindings>> {
    proptest::option::of(reasons().prop_map(|reasons| TargetFindings { reasons }))
}

fn inapplicable_row(n: usize, findings: Option<TargetFindings>) -> BatchResult {
    let pin = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v2.15.3").unwrap(),
    );
    BatchResult {
        commit: sha(n),
        series: SeriesName::new("v3.13.x").unwrap(),
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
        apply: None,
        target_findings: findings,
    }
}

fn rows() -> impl Strategy<Value = Vec<BatchResult>> {
    proptest::collection::vec(findings(), 0..8).prop_map(|per_row| {
        per_row
            .into_iter()
            .enumerate()
            .map(|(n, f)| inapplicable_row(n, f))
            .collect()
    })
}

proptest! {
    // a non-empty finding set on any row forces Findings and exit 3
    #[test]
    fn a_finding_forces_the_findings_arm(rows in rows()) {
        let clearance = RoundClearance::from_results(&rows, &BTreeSet::new());
        let any_finding = rows
            .iter()
            .any(|r| r.target_findings.as_ref().is_some_and(|t| !t.is_empty()));
        if any_finding {
            prop_assert!(matches!(clearance, RoundClearance::Findings(_)));
            prop_assert_eq!(clearance.facts().exit_code, 3);
        } else {
            prop_assert!(clearance.is_clean());
            prop_assert_eq!(clearance.facts().exit_code, 0);
        }
    }

    // adding a finding to a row never lowers the round exit code
    #[test]
    fn adding_a_finding_never_lowers_the_exit_code(rows in rows(), extra in 0usize..8) {
        let before = RoundClearance::from_results(&rows, &BTreeSet::new())
            .facts()
            .exit_code;
        let mut grown = rows;
        if grown.is_empty() {
            return Ok(());
        }
        let idx = extra % grown.len();
        let target = grown[idx]
            .target_findings
            .get_or_insert_with(TargetFindings::default);
        target.reasons.push(reason("drain", 7));
        let after = RoundClearance::from_results(&grown, &BTreeSet::new())
            .facts()
            .exit_code;
        prop_assert!(after >= before);
    }

    // by_class totals plus unclassified equal the reason count
    #[test]
    fn class_counts_partition_the_reasons(rows in rows()) {
        let facts_owner = RoundClearance::from_results(&rows, &BTreeSet::new());
        let facts = facts_owner.facts();
        let classified: usize = facts.target.by_class.iter().map(|(_, n)| n).sum();
        let expected: usize = rows
            .iter()
            .filter_map(|r| r.target_findings.as_ref())
            .map(|t| t.reasons.len())
            .sum();
        prop_assert_eq!(classified + facts.target.unclassified, expected);
    }

    #[test]
    fn the_record_round_trips_through_json(reasons in reasons()) {
        let findings = TargetFindings { reasons };
        let json = serde_json::to_string(&findings).unwrap();
        let back: TargetFindings = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, findings);
    }
}
