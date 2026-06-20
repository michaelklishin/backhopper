// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Text rendering of `check batch`. The load-bearing test is the
//! vacuous-round one: a clean round must state its negatives so it
//! reads as "checked, clean" rather than "never ran". `--summary-only`
//! keeps the terse per-row counts.

use std::collections::BTreeSet;

use backhopper_cli::cli::CheckFlags;
use backhopper_cli::commands::check::render_batch_text;
use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::names::{CommitSha, DependencyName, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    BumpStatus, Diagnostics, InapplicableReason, PatchFacts, PinBump, PinVerdict, SeriesVerdict,
    Verdict,
};

fn commit(c: char) -> CommitSha {
    CommitSha::new(c.to_string().repeat(40)).unwrap()
}

fn pin(project: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

fn inapplicable(project: &str, reason: InapplicableReason) -> PinVerdict {
    PinVerdict::new(pin(project), Verdict::Inapplicable { reason })
}

fn row(c: char, pins: Vec<PinVerdict>, diagnostics: Diagnostics) -> BatchResult {
    BatchResult {
        commit: commit(c),
        series: SeriesName::new("v4.1.x").unwrap(),
        verdict: SeriesVerdict::from_results(pins),
        diagnostics,
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
    }
}

fn render(rows: &[BatchResult], flags: CheckFlags) -> String {
    let mut buf: Vec<u8> = Vec::new();
    render_batch_text(&mut buf, rows, flags, &BTreeSet::new(), &[]).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn vacuous_round_states_its_negatives() {
    let rows = vec![
        row(
            'a',
            vec![inapplicable(
                "ra",
                InapplicableReason::NoErlangSurfaceTouched,
            )],
            Diagnostics::default(),
        ),
        row(
            'b',
            vec![inapplicable("ra", InapplicableReason::OnlyDocsTouched)],
            Diagnostics::default(),
        ),
    ];
    let out = render(&rows, CheckFlags::default());
    assert!(out.contains("clearance:"), "{out}");
    assert!(
        out.contains("tracked dep surface : 0 symbols referenced"),
        "{out}"
    );
    assert!(out.contains("dep-pin bumps       : none"), "{out}");
    assert!(out.contains("no_erlang_surface_touched"), "{out}");
    assert!(out.contains("no dependency-API risk found"), "{out}");
    assert!(out.contains("non-analyzable files"), "{out}");
    assert!(!out.contains("review the rows below"), "{out}");
    assert!(
        !out.contains("tracked refs:"),
        "clean rows stay terse: {out}"
    );
}

#[test]
fn summary_only_stays_terse() {
    let rows = vec![row(
        'a',
        vec![inapplicable(
            "ra",
            InapplicableReason::NoErlangSurfaceTouched,
        )],
        Diagnostics::default(),
    )];
    let out = render(
        &rows,
        CheckFlags {
            summary_only: true,
            ..CheckFlags::default()
        },
    );
    assert!(!out.contains("clearance:"), "{out}");
    assert!(out.contains("inapplicable=1"), "{out}");
}

#[test]
fn snapshot_missing_bump_is_flagged_and_round_needs_review() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![PinBump {
            dep: DependencyName::new("ra").unwrap(),
            from: Some("hex 2.16.0".to_owned()),
            to: "hex 2.17.0".to_owned(),
            status: Some(BumpStatus::SnapshotMissing {
                note: "snapshots generate".to_owned(),
            }),
        }],
        ..Diagnostics::default()
    };
    let rows = vec![row(
        'a',
        vec![inapplicable("ra", InapplicableReason::OnlyMakefileTouched)],
        diagnostics,
    )];
    let out = render(&rows, CheckFlags::default());
    assert!(out.contains("snapshot MISSING"), "{out}");
    assert!(out.contains("review the rows below"), "{out}");
}

#[test]
fn an_introduced_pin_renders_in_the_per_row_breadcrumb() {
    let diagnostics = Diagnostics {
        pin_bumps: vec![PinBump {
            dep: DependencyName::new("osiris").unwrap(),
            from: None,
            to: "hex 1.8.0".to_owned(),
            status: Some(BumpStatus::SnapshotPresent),
        }],
        ..Diagnostics::default()
    };
    let rows = vec![row(
        'a',
        vec![inapplicable(
            "osiris",
            InapplicableReason::OnlyMakefileTouched,
        )],
        diagnostics,
    )];
    let out = render(&rows, CheckFlags::default());
    assert!(out.contains("osiris introduced at hex 1.8.0"), "{out}");
    assert!(out.contains("1 introduced"), "{out}");
}

#[test]
fn a_tracked_reference_marks_the_round_for_review() {
    let rows = vec![row(
        'a',
        vec![PinVerdict::new(pin("ra"), Verdict::Compatible).with_tracked_refs(2)],
        Diagnostics::default(),
    )];
    let out = render(&rows, CheckFlags::default());
    assert!(
        out.contains("tracked dep surface : 2 symbols referenced"),
        "{out}"
    );
    assert!(out.contains("tracked refs: 2 (see --explain)"), "{out}");
    assert!(out.contains("review the rows below"), "{out}");
}
