// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The postimage–preimage duality property: for a generated base
//! file and an insertion patch, evaluating against the *patched*
//! tree reports the hunk already applied, and evaluating against
//! the *base* tree reports it not applied (and the preimage matches
//! exactly, so no drift reasons fire). A sign error in either
//! classifier breaks one of the two sides.

use std::fmt::Write;
use std::path::PathBuf;

use proptest::collection::vec;
use proptest::prelude::*;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::verdict::{ContentPresence, Reason, SeriesEvaluation};
use backhopper_test_support::{canonical_snapshot, snapshot_header};

const INJECTED: &str = "%% injected marker line";

fn snapshot() -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("demo", "v1.0.0"), vec![])
}

/// Full-context insertion diff: every base line is context, one
/// marker line inserted at `pos`.
fn insertion_diff(base: &[String], pos: usize) -> String {
    let mut diff = String::from(
        "diff --git a/src/demo.erl b/src/demo.erl\n--- a/src/demo.erl\n+++ b/src/demo.erl\n",
    );
    writeln!(diff, "@@ -1,{} +1,{} @@", base.len(), base.len() + 1).unwrap();
    for (i, line) in base.iter().enumerate() {
        if i == pos {
            writeln!(diff, "+{INJECTED}").unwrap();
        }
        writeln!(diff, " {line}").unwrap();
    }
    if pos == base.len() {
        writeln!(diff, "+{INJECTED}").unwrap();
    }
    diff
}

fn patched_content(base: &[String], pos: usize) -> String {
    let mut out = String::new();
    for (i, line) in base.iter().enumerate() {
        if i == pos {
            out.push_str(INJECTED);
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
    }
    if pos == base.len() {
        out.push_str(INJECTED);
        out.push('\n');
    }
    out
}

fn evaluate(diff: &str, file_content: &str) -> SeriesEvaluation {
    let snap = snapshot();
    let scope = PinScope::from_snapshot(ProjectName::new("demo").unwrap(), &snap, Vec::new());
    let files = EvaluationFiles::new().with(
        PathBuf::from("src/demo.erl"),
        Some(file_content.as_bytes().to_vec()),
    );
    let pin = Pin::new(
        ProjectName::new("demo").unwrap(),
        TagName::new("v1.0.0").unwrap(),
    );
    let ctx = EvaluationContext::new(pin, snap, scope).with_files(files);
    Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx])
}

fn content(evaluation: &SeriesEvaluation) -> Option<&ContentPresence> {
    evaluation
        .diagnostics
        .already_present
        .as_ref()
        .and_then(|ap| ap.content.as_ref())
}

proptest! {
    #[test]
    fn applied_tree_reports_applied_and_base_tree_does_not(
        // lines distinct enough that the injected marker cannot collide with base content
        base in vec("[a-z][a-z0-9_]{2,18}\\(\\) -> ok\\.", 1..12),
        pos_seed in 0usize..64,
    ) {
        let pos = pos_seed % (base.len() + 1);
        let diff = insertion_diff(&base, pos);
        let base_text = format!("{}\n", base.join("\n"));

        let on_base = evaluate(&diff, &base_text);
        let p = content(&on_base).expect("insertion hunks are considered");
        prop_assert_eq!(p.hunks_considered, 1);
        prop_assert_eq!(p.hunks_already_applied, 0);
        // the preimage is the pristine base: no drift reasons
        let reasons = on_base.verdict.results[0].verdict.reasons();
        let no_drift = reasons.iter().all(|r| {
            !matches!(
                r,
                Reason::PreimageDrifted { .. } | Reason::PreimageMissing { .. }
            )
        });
        prop_assert!(no_drift);

        let on_patched = evaluate(&diff, &patched_content(&base, pos));
        let p = content(&on_patched).expect("insertion hunks are considered");
        prop_assert_eq!(p.hunks_considered, 1);
        // the marker splits or edge-extends the context block: the hunk reports
        // applied, or ambiguous at file edges where the context stays contiguous
        prop_assert_eq!(p.hunks_already_applied + p.hunks_ambiguous, 1);
    }
}
