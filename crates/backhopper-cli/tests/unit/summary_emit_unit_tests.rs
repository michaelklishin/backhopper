// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::cli::Formatter;
use backhopper_cli::commands::summary::{SummaryFormatter, emit_rows, to_summary_row};
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
    }
}

#[test]
fn rollup_prefers_incompatible_over_other_kinds() {
    let mut compat = PinVerdict::new(fixture_pin("a"), Verdict::Compatible);
    let mut incompat = PinVerdict::new(fixture_pin("b"), Verdict::Incompatible { reasons: vec![] });
    compat.tracked_refs = 3;
    incompat.tracked_refs = 5;
    let eval = series_eval_with(vec![compat, incompat]);
    let row = to_summary_row(&eval, fixture_sha(), "hi".into());
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
    let row = to_summary_row(&eval, fixture_sha(), "hi".into());
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
    let row = to_summary_row(&eval, fixture_sha(), "hi".into());
    assert_eq!(row.verdict, VerdictKind::Inapplicable);
}

#[test]
fn tracked_saturates_at_u32_max_on_huge_sums() {
    let mut pin = PinVerdict::new(fixture_pin("a"), Verdict::Compatible);
    pin.tracked_refs = usize::MAX;
    let eval = series_eval_with(vec![pin]);
    let row = to_summary_row(&eval, fixture_sha(), "hi".into());
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
