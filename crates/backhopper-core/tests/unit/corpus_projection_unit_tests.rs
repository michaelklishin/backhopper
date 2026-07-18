// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `BatchResult::to_corpus_entry` and `predicted_conflicts`: the
//! projection a measurement consumer reads.

use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::eval::BuildOutcome;
use backhopper_core::model::evaluation::AggregateVerdict;
use backhopper_core::model::fingerprint::VerdictFingerprint;
use backhopper_core::model::names::{CommitSha, ProjectName, RelativePath, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::resolver_coverage::{ResolverClass, ResolverCoverage};
use backhopper_core::model::verdict::{
    ApplyConflictKind, Diagnostics, PatchFacts, PinVerdict, Reason, SeriesVerdict, Verdict,
};

fn pin() -> Pin {
    Pin::new(
        ProjectName::new("rabbit").unwrap(),
        TagName::new("v4.1.0").unwrap(),
    )
}

fn row(verdict: Verdict, fingerprint: Option<&str>) -> BatchResult {
    BatchResult {
        commit: CommitSha::new("a".repeat(40)).unwrap(),
        series: SeriesName::new("v4.1.x").unwrap(),
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(pin(), verdict)]),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: fingerprint.map(VerdictFingerprint::new),
        apply: None,
        target_findings: None,
    }
}

fn empty_row(fingerprint: Option<&str>) -> BatchResult {
    let mut r = row(Verdict::Compatible, fingerprint);
    r.verdict = SeriesVerdict::from_results(Vec::new());
    r
}

fn macro_undefined() -> Reason {
    Reason::MacroUndefinedOnTarget {
        source_path: RelativePath::new("src/osiris_log.erl").unwrap(),
        macro_name: "SEGMENT_MAX_BYTES".to_owned(),
        line: 1,
    }
}

// The break class comes from the observed outcome, never the predicted reason.
#[test]
fn to_corpus_entry_records_the_observed_break_class_not_the_predicted() {
    let verdict = Verdict::RequiresAdaptation {
        reasons: vec![macro_undefined()],
    };
    let entry = row(verdict, Some("fp1"))
        .to_corpus_entry(
            BuildOutcome::CompilationFailed {
                class: Some(ResolverClass::LocalCall),
            },
            None,
        )
        .expect("a fingerprinted, non-empty row is measurable");
    assert_eq!(entry.outcome.break_class(), Some(ResolverClass::LocalCall));
    assert_eq!(entry.verdict, AggregateVerdict::RequiresAdaptation);
    assert_eq!(entry.fingerprint, VerdictFingerprint::new("fp1"));
}

#[test]
fn to_corpus_entry_is_none_without_a_fingerprint() {
    let entry = row(Verdict::Compatible, None).to_corpus_entry(BuildOutcome::BuiltClean, None);
    assert!(entry.is_none());
}

// An empty verdict predicted nothing: not a measurable row even with a fingerprint.
#[test]
fn to_corpus_entry_is_none_for_an_empty_verdict() {
    let entry = empty_row(Some("fp1")).to_corpus_entry(BuildOutcome::BuiltClean, None);
    assert!(entry.is_none());
}

#[test]
fn to_corpus_entry_threads_the_round_coverage() {
    let cov = ResolverCoverage::current();
    let entry = row(Verdict::Compatible, Some("fp1"))
        .to_corpus_entry(BuildOutcome::BuiltClean, Some(&cov))
        .expect("measurable");
    assert_eq!(entry.coverage, Some(cov));
}

fn preimage_missing(path: &str) -> Reason {
    Reason::PreimageMissing {
        path: path.into(),
        hunk_index: 0,
        preimage_excerpt: String::new(),
    }
}

#[test]
fn predicted_conflicts_lists_only_risky_paths() {
    let verdict = Verdict::RequiresAdaptation {
        reasons: vec![
            preimage_missing("src/ra_server.erl"),
            Reason::PreimageDrifted {
                path: "src/ra_server.erl".into(),
                hunk_index: 1,
                line_delta: 2,
            },
        ],
    };
    let conflicts = row(verdict, None).predicted_conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path.to_str(), Some("src/ra_server.erl"));
    assert_eq!(conflicts[0].kind, ApplyConflictKind::PreimageMissing);
}

// Two kinds on one path collapse to the higher severity.
#[test]
fn predicted_conflicts_dedups_to_the_max_severity() {
    let verdict = Verdict::RequiresAdaptation {
        reasons: vec![
            Reason::PostimageCollision {
                path: "src/ra_server.erl".into(),
                hunk_index: 0,
            },
            Reason::FileAbsent {
                path: "src/ra_server.erl".into(),
            },
        ],
    };
    let conflicts = row(verdict, None).predicted_conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].kind, ApplyConflictKind::FileAbsent);
}

#[test]
fn predicted_conflicts_keeps_one_entry_per_path() {
    let verdict = Verdict::RequiresAdaptation {
        reasons: vec![
            preimage_missing("src/ra_server.erl"),
            Reason::FileAbsent {
                path: "src/ra_log.erl".into(),
            },
        ],
    };
    let conflicts = row(verdict, None).predicted_conflicts();
    assert_eq!(conflicts.len(), 2);
    // predicted_conflicts is keyed by path, so entries come out path-sorted
    assert_eq!(conflicts[0].path.to_str(), Some("src/ra_log.erl"));
    assert_eq!(conflicts[1].path.to_str(), Some("src/ra_server.erl"));
}
