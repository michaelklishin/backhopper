// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `RoundClearance::from_results` roll-up: the Clean-versus-Findings
//! discriminator, the inapplicability histogram, self-pin exclusion,
//! the three-way bump classification with cross-series dedup, and the
//! suite union.

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::model::apply::{ApplyForecast, PathApplyOutcome, UnassessedReason};
use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::clearance::RoundClearance;
use backhopper_core::model::findings::TargetFindings;
use backhopper_core::model::names::{
    Arity, CommitSha, DependencyName, FunctionName, ModuleName, ProjectName, RelativePath,
    SeriesName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::resolver_coverage::ResolverClass;
use backhopper_core::model::verdict::{
    ApplyConflictKind, BumpStatus, Diagnostics, InapplicableReason, IndirectCallForm, PatchFacts,
    PinBump, PinVerdict, Reason, SeriesVerdict, Verdict,
};

fn commit(c: char) -> CommitSha {
    CommitSha::new(c.to_string().repeat(40)).unwrap()
}

fn proj(name: &str) -> ProjectName {
    ProjectName::new(name).unwrap()
}

fn pin(project: &str) -> Pin {
    Pin::new(proj(project), TagName::new("v1.0.0").unwrap())
}

fn tracked_pin(project: &str, verdict: Verdict, tracked: usize) -> PinVerdict {
    PinVerdict::new(pin(project), verdict).with_tracked_refs(tracked)
}

fn inapplicable(project: &str, reason: InapplicableReason) -> PinVerdict {
    PinVerdict::new(pin(project), Verdict::Inapplicable { reason })
}

fn bump(dep: &str, from: Option<&str>, to: &str, status: Option<BumpStatus>) -> PinBump {
    PinBump {
        dep: DependencyName::new(dep).unwrap(),
        from: from.map(str::to_owned),
        to: to.to_owned(),
        status,
    }
}

fn row(c: char, series: &str, pins: Vec<PinVerdict>) -> BatchResult {
    row_with_diagnostics(c, series, pins, Diagnostics::default())
}

fn row_with_diagnostics(
    c: char,
    series: &str,
    pins: Vec<PinVerdict>,
    diagnostics: Diagnostics,
) -> BatchResult {
    BatchResult {
        commit: commit(c),
        series: SeriesName::new(series).unwrap(),
        verdict: SeriesVerdict::from_results(pins),
        diagnostics,
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    }
}

fn no_self() -> BTreeSet<ProjectName> {
    BTreeSet::new()
}

// An unresolved intra-repo macro forces Findings even with zero tracked references.
#[test]
fn a_macro_undefined_finding_forces_findings_with_zero_tracked() {
    let verdict = Verdict::RequiresAdaptation {
        reasons: vec![Reason::MacroUndefinedOnTarget {
            source_path: RelativePath::new("deps/rabbit/src/x.erl").unwrap(),
            macro_name: "OAUTH2_BOOTSTRAP_PATH".to_owned(),
            line: 1,
        }],
    };
    let rows = [row('a', "v4.1.x", vec![tracked_pin("rabbit", verdict, 0)])];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(!clearance.is_clean());
    assert_eq!(clearance.facts().tracked, 0);
}

#[test]
fn empty_round_is_clean() {
    let clearance = RoundClearance::from_results(&[], &no_self());
    assert!(clearance.is_clean());
    let facts = clearance.facts();
    assert_eq!(facts.rows, 0);
    assert_eq!(facts.candidates, 0);
    assert_eq!(facts.series, 0);
    assert_eq!(facts.tracked, 0);
    assert_eq!(facts.exit_code, 0);
    assert!(facts.reasons.is_empty());
    assert!(facts.bumps.is_empty());
}

#[test]
fn vacuous_inapplicable_round_is_clean_with_reason_histogram() {
    let rows = vec![
        row(
            'a',
            "v4.1.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::NoErlangSurfaceTouched,
            )],
        ),
        row(
            'b',
            "v4.1.x",
            vec![inapplicable("khepri", InapplicableReason::OnlyDocsTouched)],
        ),
        row(
            'c',
            "v4.1.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::NoErlangSurfaceTouched,
            )],
        ),
    ];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(clearance.is_clean());
    let facts = clearance.facts();
    assert_eq!(facts.candidates, 3);
    assert_eq!(facts.series, 1);
    assert_eq!(facts.tracked, 0);
    assert_eq!(facts.verdicts.inapplicable, 3);
    assert_eq!(facts.reasons.count("no_erlang_surface_touched"), 2);
    assert_eq!(facts.reasons.count("only_docs_touched"), 1);
    assert_eq!(facts.reasons.total(), 3);
}

#[test]
fn a_tracked_reference_makes_the_round_findings() {
    let rows = vec![row(
        'a',
        "s",
        vec![tracked_pin("ra", Verdict::Compatible, 2)],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(!clearance.is_clean());
    assert_eq!(clearance.facts().tracked, 2);
    assert_eq!(clearance.facts().exit_code, 0);
}

#[test]
fn incompatible_is_findings_and_needs_attention() {
    let rows = vec![row(
        'a',
        "s",
        vec![tracked_pin(
            "ra",
            Verdict::Incompatible {
                reasons: Vec::new(),
            },
            0,
        )],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(!clearance.is_clean());
    assert_eq!(clearance.facts().exit_code, 3);
    assert_eq!(clearance.facts().verdicts.incompatible, 1);
}

#[test]
fn requires_adaptation_is_findings_and_needs_attention() {
    let rows = vec![row(
        'a',
        "s",
        vec![tracked_pin(
            "ra",
            Verdict::RequiresAdaptation {
                reasons: Vec::new(),
            },
            0,
        )],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(!clearance.is_clean());
    assert_eq!(clearance.facts().exit_code, 3);
    assert_eq!(clearance.facts().verdicts.requires_adaptation, 1);
}

#[test]
fn tracked_count_excludes_self_pins() {
    let mut self_projects = BTreeSet::new();
    self_projects.insert(proj("rabbit"));
    let rows = vec![row(
        'a',
        "s",
        vec![
            tracked_pin("rabbit", Verdict::Compatible, 5),
            tracked_pin("ra", Verdict::Compatible, 3),
        ],
    )];
    let clearance = RoundClearance::from_results(&rows, &self_projects);
    assert_eq!(clearance.facts().tracked, 3);
    assert!(!clearance.is_clean());
}

#[test]
fn a_round_with_only_self_pin_references_is_clean() {
    let mut self_projects = BTreeSet::new();
    self_projects.insert(proj("rabbit"));
    let rows = vec![row(
        'a',
        "s",
        vec![tracked_pin("rabbit", Verdict::Compatible, 5)],
    )];
    let clearance = RoundClearance::from_results(&rows, &self_projects);
    assert_eq!(clearance.facts().tracked, 0);
    assert!(clearance.is_clean());
}

#[test]
fn snapshot_missing_bump_makes_findings() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![bump(
            "ra",
            Some("hex 2.16.0"),
            "hex 2.17.0",
            Some(BumpStatus::SnapshotMissing {
                note: "snapshots generate".to_owned(),
            }),
        )],
        ..Default::default()
    };
    let rows = vec![row_with_diagnostics(
        'a',
        "s",
        vec![inapplicable("ra", InapplicableReason::OnlyMakefileTouched)],
        diagnostics,
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(!clearance.is_clean());
    let facts = clearance.facts();
    assert_eq!(facts.bumps.total, 1);
    assert_eq!(facts.bumps.snapshot_missing, 1);
}

#[test]
fn snapshot_present_bump_alone_stays_clean() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![bump(
            "ra",
            Some("hex 2.16.0"),
            "hex 2.17.0",
            Some(BumpStatus::SnapshotPresent),
        )],
        ..Default::default()
    };
    let rows = vec![row_with_diagnostics(
        'a',
        "s",
        vec![inapplicable("ra", InapplicableReason::OnlyMakefileTouched)],
        diagnostics,
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(clearance.is_clean());
    let facts = clearance.facts();
    assert_eq!(facts.bumps.total, 1);
    assert_eq!(facts.bumps.snapshot_present, 1);
    assert_eq!(facts.bumps.snapshot_missing, 0);
}

#[test]
fn untracked_and_unassessed_bumps_are_classified_and_do_not_force_findings() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![
            bump("elixir", Some("1.16"), "1.17", Some(BumpStatus::Untracked)),
            bump("ra", Some("hex 1.0.0"), "hex 2.0.0", None),
        ],
        ..Default::default()
    };
    let rows = vec![row_with_diagnostics(
        'a',
        "s",
        vec![inapplicable("ra", InapplicableReason::OnlyMakefileTouched)],
        diagnostics,
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(clearance.is_clean());
    let facts = clearance.facts();
    assert_eq!(facts.bumps.total, 2);
    assert_eq!(facts.bumps.untracked, 1);
    assert_eq!(facts.bumps.unassessed, 1);
    assert_eq!(facts.bumps.snapshot_missing, 0);
}

#[test]
fn the_same_bump_under_several_series_counts_once() {
    let bumped = |series: &str| {
        let diagnostics = Diagnostics {
            pin_bumps: vec![bump(
                "ra",
                Some("hex 1.0.0"),
                "hex 2.0.0",
                Some(BumpStatus::SnapshotPresent),
            )],
            ..Default::default()
        };
        row_with_diagnostics(
            'a',
            series,
            vec![tracked_pin("ra", Verdict::Compatible, 0)],
            diagnostics,
        )
    };
    let rows = vec![bumped("s1"), bumped("s2")];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    let facts = clearance.facts();
    assert_eq!(facts.bumps.total, 1);
    assert_eq!(facts.candidates, 1);
    assert_eq!(facts.series, 2);
}

#[test]
fn the_same_bump_under_distinct_commits_counts_twice() {
    let bumped = |c: char| {
        let diagnostics = Diagnostics {
            pin_bumps: vec![bump(
                "ra",
                Some("hex 1.0.0"),
                "hex 2.0.0",
                Some(BumpStatus::SnapshotPresent),
            )],
            ..Default::default()
        };
        row_with_diagnostics(
            c,
            "s",
            vec![tracked_pin("ra", Verdict::Compatible, 0)],
            diagnostics,
        )
    };
    let rows = vec![bumped('a'), bumped('b')];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert_eq!(clearance.facts().bumps.total, 2);
}

#[test]
fn an_introduced_pin_is_counted() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![bump(
            "osiris",
            None,
            "hex 1.8.0",
            Some(BumpStatus::SnapshotPresent),
        )],
        ..Default::default()
    };
    let rows = vec![row_with_diagnostics(
        'a',
        "s",
        vec![inapplicable(
            "osiris",
            InapplicableReason::OnlyMakefileTouched,
        )],
        diagnostics,
    )];
    let facts = RoundClearance::from_results(&rows, &no_self())
        .facts()
        .clone();
    assert_eq!(facts.bumps.introduced, 1);
    assert_eq!(facts.bumps.total, 1);
}

#[test]
fn suggested_suites_are_unioned_sorted_and_deduplicated() {
    let with_suites = |series: &str, suites: &[&str]| {
        let diagnostics = Diagnostics {
            suggested_suites: suites.iter().map(|s| (*s).to_owned()).collect(),
            ..Default::default()
        };
        row_with_diagnostics(
            'a',
            series,
            vec![inapplicable("ra", InapplicableReason::OnlyDocsTouched)],
            diagnostics,
        )
    };
    let rows = vec![
        with_suites("s1", &["b_SUITE", "a_SUITE"]),
        with_suites("s2", &["a_SUITE", "c_SUITE"]),
    ];
    let facts = RoundClearance::from_results(&rows, &no_self())
        .facts()
        .clone();
    assert_eq!(facts.suites, vec!["a_SUITE", "b_SUITE", "c_SUITE"]);
}

#[test]
fn reason_histogram_ranks_by_count_then_label() {
    let rows = vec![row(
        'a',
        "s",
        vec![
            inapplicable("p1", InapplicableReason::NoErlangSurfaceTouched),
            inapplicable("p2", InapplicableReason::NoErlangSurfaceTouched),
            inapplicable("p3", InapplicableReason::NoErlangSurfaceTouched),
            inapplicable("p4", InapplicableReason::OnlyMakefileTouched),
            inapplicable("p5", InapplicableReason::OnlyMakefileTouched),
            inapplicable("p6", InapplicableReason::OnlyDocsTouched),
        ],
    )];
    let facts = RoundClearance::from_results(&rows, &no_self())
        .facts()
        .clone();
    let ranked = facts.reasons.ranked();
    assert_eq!(
        ranked,
        vec![
            ("no_erlang_surface_touched", 3),
            ("only_makefile_touched", 2),
            ("only_docs_touched", 1),
        ]
    );
}

#[test]
fn ranked_breaks_count_ties_alphabetically() {
    let rows = vec![row(
        'a',
        "s",
        vec![
            inapplicable("p1", InapplicableReason::OnlyMakefileTouched),
            inapplicable("p2", InapplicableReason::OnlyDocsTouched),
        ],
    )];
    let facts = RoundClearance::from_results(&rows, &no_self())
        .facts()
        .clone();
    let ranked = facts.reasons.ranked();
    assert_eq!(
        ranked,
        vec![("only_docs_touched", 1), ("only_makefile_touched", 1)]
    );
}

#[test]
fn verdict_totals_sum_per_pin_counts() {
    let rows = vec![row(
        'a',
        "s",
        vec![
            tracked_pin("p1", Verdict::Compatible, 0),
            tracked_pin(
                "p2",
                Verdict::Incompatible {
                    reasons: Vec::new(),
                },
                0,
            ),
            inapplicable("p3", InapplicableReason::OnlyDocsTouched),
        ],
    )];
    let facts = RoundClearance::from_results(&rows, &no_self())
        .facts()
        .clone();
    assert_eq!(facts.verdicts.compatible, 1);
    assert_eq!(facts.verdicts.incompatible, 1);
    assert_eq!(facts.verdicts.inapplicable, 1);
    assert_eq!(facts.verdicts.requires_adaptation, 0);
}

#[test]
fn all_inapplicable_is_zero_domain() {
    let rows = vec![
        row(
            'a',
            "v4.1.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::NoErlangSurfaceTouched,
            )],
        ),
        row(
            'b',
            "v4.1.x",
            vec![inapplicable("khepri", InapplicableReason::OnlyDocsTouched)],
        ),
    ];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    assert!(clearance.is_clean());
}

#[test]
fn mixed_inapplicable_and_compatible_is_clean() {
    let rows = vec![row(
        'a',
        "v4.1.x",
        vec![
            inapplicable("ra", InapplicableReason::OnlyDocsTouched),
            tracked_pin("khepri", Verdict::Compatible, 0),
        ],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Clean(_)));
    assert!(clearance.is_clean());
}

#[test]
fn all_inapplicable_with_snapshot_missing_bump_is_findings() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![bump(
            "ra",
            Some("hex 2.16.0"),
            "hex 2.17.0",
            Some(BumpStatus::SnapshotMissing {
                note: "snapshots generate".to_owned(),
            }),
        )],
        ..Default::default()
    };
    let rows = vec![row_with_diagnostics(
        'a',
        "v4.1.x",
        vec![inapplicable("ra", InapplicableReason::OnlyMakefileTouched)],
        diagnostics,
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    assert!(!clearance.is_clean());
}

#[test]
fn all_inapplicable_with_tracked_refs_is_findings() {
    let rows = vec![row(
        'a',
        "v4.1.x",
        vec![tracked_pin(
            "ra",
            Verdict::Inapplicable {
                reason: InapplicableReason::NoErlangSurfaceTouched,
            },
            3,
        )],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    assert!(!clearance.is_clean());
}

#[test]
fn compatible_zero_tracked_is_clean() {
    let rows = vec![row(
        'a',
        "v4.1.x",
        vec![tracked_pin("ra", Verdict::Compatible, 0)],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Clean(_)));
    assert!(clearance.is_clean());
}

#[test]
fn empty_batch_is_clean() {
    let clearance = RoundClearance::from_results(&[], &no_self());
    assert!(matches!(clearance, RoundClearance::Clean(_)));
    assert!(clearance.is_clean());
}

fn forecast_with(paths: &[(&str, PathApplyOutcome)]) -> ApplyForecast {
    let mut forecast = ApplyForecast::default();
    for (path, outcome) in paths {
        forecast.record(
            RelativePath::new((*path).to_owned()).unwrap(),
            outcome.clone(),
        );
    }
    forecast
}

fn row_with_apply(
    c: char,
    series: &str,
    pins: Vec<PinVerdict>,
    apply: Option<ApplyForecast>,
) -> BatchResult {
    let mut r = row(c, series, pins);
    r.apply = apply;
    r
}

// Every verdict inapplicable yet the forecast predicts conflicts: not ZeroDomain.
#[test]
fn all_inapplicable_with_a_forecast_conflict_is_findings_and_exits_nonzero() {
    let forecast = forecast_with(&[
        (
            "deps/rabbit/test/quorum_queue_SUITE.erl",
            PathApplyOutcome::Conflict {
                kind: ApplyConflictKind::PreimageMissing,
            },
        ),
        (
            "deps/rabbit/src/rabbit_fifo.erl",
            PathApplyOutcome::CleanExact,
        ),
    ]);
    let rows = vec![
        row_with_apply(
            'a',
            "v4.0.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::OnlyTestFixturesTouched,
            )],
            Some(forecast),
        ),
        row_with_apply(
            'b',
            "v4.0.x",
            vec![inapplicable("ra", InapplicableReason::OnlyDocsTouched)],
            Some(ApplyForecast::default()),
        ),
    ];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    assert!(!clearance.is_clean());
    let facts = clearance.facts();
    assert_eq!(facts.exit_code, 3);
    assert_eq!(facts.apply.rows_with_forecast, 2);
    assert_eq!(facts.apply.conflicted_rows, 1);
    assert_eq!(
        facts.apply.conflicted_paths,
        vec![RelativePath::new("deps/rabbit/test/quorum_queue_SUITE.erl".to_owned()).unwrap()]
    );
    assert_eq!(facts.apply.by_kind.get("preimage_missing"), Some(&1));
}

#[test]
fn all_inapplicable_with_clean_forecasts_stays_zero_domain() {
    let forecast = forecast_with(&[(
        "deps/rabbit/test/quorum_queue_SUITE.erl",
        PathApplyOutcome::CleanDrifted { line_delta: 5 },
    )]);
    let rows = vec![row_with_apply(
        'a',
        "v4.0.x",
        vec![inapplicable(
            "ra",
            InapplicableReason::OnlyTestFixturesTouched,
        )],
        Some(forecast),
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    let facts = clearance.facts();
    assert_eq!(facts.exit_code, 0);
    assert_eq!(facts.apply.rows_with_forecast, 1);
    assert_eq!(facts.apply.conflicted_rows, 0);
}

// An unassessed path is coverage, not risk: it must not flip the arm.
#[test]
fn unassessed_paths_are_counted_but_never_findings() {
    let forecast = forecast_with(&[(
        "deps/rabbit/priv/logo.png",
        PathApplyOutcome::Unassessed {
            reason: UnassessedReason::BinaryFile,
        },
    )]);
    let rows = vec![row_with_apply(
        'a',
        "v4.0.x",
        vec![inapplicable("ra", InapplicableReason::OnlyDocsTouched)],
        Some(forecast),
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    assert_eq!(clearance.facts().apply.unassessed_paths, 1);
}

#[test]
fn rows_without_forecasts_report_zero_forecast_coverage() {
    let rows = vec![row(
        'a',
        "v4.1.x",
        vec![tracked_pin("ra", Verdict::Compatible, 0)],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Clean(_)));
    assert_eq!(clearance.facts().apply.rows_with_forecast, 0);
    assert!(!clearance.facts().apply.has_conflict());
}

#[test]
fn conflicted_paths_dedup_across_rows_and_kinds_sum() {
    let suite_conflict = |kind| {
        forecast_with(&[(
            "deps/rabbit/test/quorum_queue_SUITE.erl",
            PathApplyOutcome::Conflict { kind },
        )])
    };
    let rows = vec![
        row_with_apply(
            'a',
            "v4.0.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::OnlyTestFixturesTouched,
            )],
            Some(suite_conflict(ApplyConflictKind::PreimageMissing)),
        ),
        row_with_apply(
            'a',
            "v3.13.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::OnlyTestFixturesTouched,
            )],
            Some(suite_conflict(ApplyConflictKind::FileAbsent)),
        ),
    ];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    let facts = clearance.facts();
    assert_eq!(facts.apply.conflicted_rows, 2);
    assert_eq!(facts.apply.conflicted_paths.len(), 1);
    assert_eq!(facts.apply.by_kind.get("preimage_missing"), Some(&1));
    assert_eq!(facts.apply.by_kind.get("file_absent"), Some(&1));
}

fn indirect_finding(function: &str) -> Reason {
    Reason::IndirectCallUndefinedOnTarget {
        source_path: RelativePath::new("deps/rabbit/test/maintenance_mode_SUITE.erl").unwrap(),
        module: ModuleName::from_str("rabbit_queue_type").unwrap(),
        function: FunctionName::from_str(function).unwrap(),
        arity: Arity::new(1),
        via: IndirectCallForm::MeckExpect,
        line: 356,
    }
}

fn row_with_findings(
    c: char,
    series: &str,
    pins: Vec<PinVerdict>,
    findings: Option<TargetFindings>,
) -> BatchResult {
    let mut r = row(c, series, pins);
    r.target_findings = findings;
    r
}

// Every verdict inapplicable yet the target checks found missing symbols: not ZeroDomain.
#[test]
fn all_inapplicable_with_a_target_finding_is_findings_and_exits_nonzero() {
    let rows = vec![
        row_with_findings(
            'a',
            "v3.13.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::OnlyTestFixturesTouched,
            )],
            Some(TargetFindings {
                reasons: vec![indirect_finding("drain"), indirect_finding("revive")],
            }),
        ),
        row_with_findings(
            'b',
            "v3.13.x",
            vec![inapplicable("ra", InapplicableReason::OnlyDocsTouched)],
            Some(TargetFindings::default()),
        ),
    ];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    let facts = clearance.facts();
    assert_eq!(facts.exit_code, 3);
    assert_eq!(facts.target.rows_with_findings, 2);
    assert_eq!(facts.target.finding_rows, 1);
    assert_eq!(
        facts.target.by_class,
        vec![(ResolverClass::IndirectCall, 2)]
    );
    assert_eq!(facts.target.unclassified, 0);
}

#[test]
fn all_inapplicable_with_empty_findings_stays_zero_domain() {
    let rows = vec![row_with_findings(
        'a',
        "v3.13.x",
        vec![inapplicable(
            "ra",
            InapplicableReason::OnlyTestFixturesTouched,
        )],
        Some(TargetFindings::default()),
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    let facts = clearance.facts();
    assert_eq!(facts.exit_code, 0);
    assert_eq!(facts.target.rows_with_findings, 1);
    assert_eq!(facts.target.finding_rows, 0);
}

#[test]
fn rows_without_the_findings_field_keep_todays_arms() {
    let rows = vec![row(
        'a',
        "v3.13.x",
        vec![inapplicable(
            "ra",
            InapplicableReason::OnlyTestFixturesTouched,
        )],
    )];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    assert_eq!(clearance.facts().target.rows_with_findings, 0);
}

// both target axes at once produce one Findings with both rollups
#[test]
fn a_forecast_conflict_and_a_target_finding_populate_both_rollups() {
    let forecast = forecast_with(&[(
        "deps/rabbit/test/quorum_queue_SUITE.erl",
        PathApplyOutcome::Conflict {
            kind: ApplyConflictKind::PreimageMissing,
        },
    )]);
    let rows = vec![
        row_with_apply(
            'a',
            "v3.13.x",
            vec![inapplicable(
                "ra",
                InapplicableReason::OnlyTestFixturesTouched,
            )],
            Some(forecast),
        ),
        row_with_findings(
            'b',
            "v3.13.x",
            vec![inapplicable("ra", InapplicableReason::OnlyDocsTouched)],
            Some(TargetFindings {
                reasons: vec![indirect_finding("drain")],
            }),
        ),
    ];
    let clearance = RoundClearance::from_results(&rows, &no_self());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    let facts = clearance.facts();
    assert_eq!(facts.apply.conflicted_rows, 1);
    assert_eq!(facts.target.finding_rows, 1);
}
