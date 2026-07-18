// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::cli::Formatter;
use std::num::NonZeroU32;

use std::collections::BTreeSet;

use backhopper_cli::commands::summary::{
    SummaryFormatter, batch_result_to_summary_row, emit_rows, to_summary_row,
};
use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::summary::{SummaryRow, VerdictKind};
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, PatchFacts, PinVerdict, SeriesEvaluation, SeriesVerdict,
    TouchedKinds, Verdict,
};

fn series_eval_with(pins: Vec<PinVerdict>) -> SeriesEvaluation {
    SeriesEvaluation {
        verdict: SeriesVerdict::from_results(pins),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        apply: None,
        target_findings: None,
    }
}

#[test]
fn rollup_prefers_incompatible_over_other_kinds() {
    let mut compat = PinVerdict::new(fixture_pin("a"), Verdict::Compatible);
    let mut incompat = PinVerdict::new(fixture_pin("b"), Verdict::Incompatible { reasons: vec![] });
    compat.tracked_refs = 3;
    incompat.tracked_refs = 5;
    let eval = series_eval_with(vec![compat, incompat]);
    let row = to_summary_row(
        &eval,
        &BTreeSet::new(),
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    assert_eq!(row.verdict, VerdictKind::Incompatible);
    assert_eq!(row.tracked, 8);
}

#[test]
fn rollup_promotes_requires_adaptation_over_compatible() {
    let eval = series_eval_with(vec![
        PinVerdict::new(fixture_pin("a"), Verdict::Compatible),
        PinVerdict::new(
            fixture_pin("b"),
            Verdict::RequiresAdaptation { reasons: vec![] },
        ),
    ]);
    let row = to_summary_row(
        &eval,
        &BTreeSet::new(),
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    assert_eq!(row.verdict, VerdictKind::RequiresAdaptation);
}

#[test]
fn rollup_substantive_kind_when_all_inapplicable() {
    let eval = series_eval_with(vec![PinVerdict::new(
        fixture_pin("a"),
        Verdict::Inapplicable {
            reason: InapplicableReason::Untracked,
        },
    )]);
    let row = to_summary_row(
        &eval,
        &BTreeSet::new(),
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    assert_eq!(row.verdict, VerdictKind::Inapplicable);
}

#[test]
fn tracked_saturates_at_u32_max_on_huge_sums() {
    let mut pin = PinVerdict::new(fixture_pin("a"), Verdict::Compatible);
    pin.tracked_refs = usize::MAX;
    let eval = series_eval_with(vec![pin]);
    let row = to_summary_row(
        &eval,
        &BTreeSet::new(),
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    assert_eq!(row.tracked, u32::MAX);
}

#[test]
fn emit_rows_text_summary_smoke_runs() {
    let row = SummaryRow {
        sha: fixture_sha(),
        verdict: VerdictKind::Compatible,
        touched: TouchedKinds {
            erl: 2,
            ..TouchedKinds::default()
        },
        tracked: 0,
        subject: "Hello".to_owned(),
        series: None,
        parent_count: None,
        apply_conflicts: 0,
        target_findings: 0,
    };
    emit_rows(SummaryFormatter::Text, &[row]).expect("emit ok");
}

#[test]
fn summary_formatter_from_cli_only_succeeds_for_summary_variants() {
    assert!(matches!(
        SummaryFormatter::from_cli(Formatter::Summary),
        Some(SummaryFormatter::Jsonl)
    ));
    assert!(matches!(
        SummaryFormatter::from_cli(Formatter::TextSummary),
        Some(SummaryFormatter::Text)
    ));
    assert!(SummaryFormatter::from_cli(Formatter::Json).is_none());
    assert!(SummaryFormatter::from_cli(Formatter::Text).is_none());
    assert!(SummaryFormatter::from_cli(Formatter::Markdown).is_none());
}

fn fixture_sha() -> CommitSha {
    CommitSha::new("a".repeat(40)).unwrap()
}

fn fixture_pin(name: &str) -> Pin {
    Pin {
        project: ProjectName::new(name).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
    }
}

#[test]
fn batch_row_projection_carries_series_and_parent_count() {
    let eval = series_eval_with(vec![
        PinVerdict::new(fixture_pin("a"), Verdict::Compatible),
        PinVerdict::new(
            fixture_pin("b"),
            Verdict::RequiresAdaptation { reasons: vec![] },
        ),
    ]);
    let result = BatchResult {
        commit: fixture_sha(),
        series: "stable".parse().unwrap(),
        verdict: eval.verdict,
        diagnostics: eval.diagnostics,
        patch_facts: eval.patch_facts,
        touched_paths: vec![],
        pr_commits: None,
        parent_count: NonZeroU32::new(2),
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    };
    let row = batch_result_to_summary_row(&result, &BTreeSet::new(), "merge subject".into());
    assert_eq!(row.sha, fixture_sha());
    assert_eq!(row.verdict, VerdictKind::RequiresAdaptation);
    assert_eq!(row.series, Some("stable".parse().unwrap()));
    assert_eq!(row.parent_count, NonZeroU32::new(2));
    assert_eq!(row.subject, "merge subject");
}

// a self pin spans the whole self project: summing it would make self-heavy commits read as harness-verified
#[test]
fn tracked_sums_non_self_pins_only() {
    let mut self_pin = PinVerdict::new(fixture_pin("rabbit"), Verdict::Compatible);
    let mut dep_pin = PinVerdict::new(fixture_pin("cowlib"), Verdict::Compatible);
    self_pin.tracked_refs = 40;
    dep_pin.tracked_refs = 2;
    let eval = series_eval_with(vec![self_pin, dep_pin]);
    let mut self_projects = BTreeSet::new();
    self_projects.insert(ProjectName::new("rabbit").unwrap());
    let row = to_summary_row(
        &eval,
        &self_projects,
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    assert_eq!(row.tracked, 2);
}

// a vacuous compatible row and a verified one must be distinguishable from the count alone
#[test]
fn vacuous_and_verified_compatible_rows_differ_in_tracked() {
    let mut verified = PinVerdict::new(fixture_pin("cowlib"), Verdict::Compatible);
    verified.tracked_refs = 3;
    let verified_row = to_summary_row(
        &series_eval_with(vec![verified]),
        &BTreeSet::new(),
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    let vacuous_row = to_summary_row(
        &series_eval_with(vec![PinVerdict::new(
            fixture_pin("cowlib"),
            Verdict::Compatible,
        )]),
        &BTreeSet::new(),
        fixture_sha(),
        "hi".into(),
        None,
        None,
    );
    assert_eq!(verified_row.verdict, vacuous_row.verdict);
    assert_eq!(verified_row.tracked, 3);
    assert_eq!(vacuous_row.tracked, 0);
}
