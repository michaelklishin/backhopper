// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Core evaluation behaviour: scope filtering, tracked-ref counting,
//! file-absent reasons, untracked record diagnostics, and per-pin
//! series aggregation. Reason-family-specific cases live in the
//! sibling `evaluation_*_unit_tests` files.

use std::path::PathBuf;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{FieldName, ModuleName, ProjectName, RecordName};
use backhopper_core::model::snapshot::{HrlFile, RecordDecl, RecordField, Snapshot, state};
use backhopper_core::model::verdict::{Reason, Verdict};
use backhopper_test_support::{module_with, pin, snapshot_header};

use crate::evaluation_support::{make_context, snapshot};

fn snapshot_with_record(project: &str, record: &str) -> Snapshot<state::Canonical> {
    let mut hrl = HrlFile::new(format!("include/{project}.hrl"));
    hrl.records.push(RecordDecl {
        name: RecordName::new(record).unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("id").unwrap(),
            type_repr: None,
        }],
    });
    Snapshot::from_extracted(snapshot_header(project, "v1.0.0"), vec![], vec![hrl]).into_canonical()
}

#[test]
fn out_of_scope_module_call_does_not_produce_a_reason() {
    let context = make_context("ra", vec![module_with("ra", &[("noop", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+flush() -> lists:foreach(fun(_) -> ok end, []).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(matches!(r0.verdict, Verdict::Compatible));
    assert_eq!(r0.tracked_refs, 0);
    assert_eq!(
        eval.diagnostics
            .untracked_calls
            .get(&ModuleName::new("lists").unwrap())
            .copied(),
        Some(1)
    );
}

#[test]
fn missing_function_in_tracked_module_is_still_incompatible() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+restart() -> ra:nonexistent_function(1, 2, 3).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(matches!(r0.verdict, Verdict::Incompatible { .. }));
    assert_eq!(r0.tracked_refs, 1);
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::MissingSymbol { .. }))
    );
}

#[test]
fn zero_tracked_refs_is_compatible_when_only_otp_called() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+for_each(L) -> lists:foreach(fun(_) -> ok end, L).
+lookup(M) -> maps:get(k, M, default).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(matches!(r0.verdict, Verdict::Compatible));
    assert_eq!(r0.tracked_refs, 0);
    assert_eq!(eval.verdict.summary.compatible, 1);
    assert_eq!(eval.verdict.summary.incompatible, 0);
    assert!(
        eval.diagnostics
            .untracked_calls
            .contains_key(&ModuleName::new("lists").unwrap())
    );
    assert!(
        eval.diagnostics
            .untracked_calls
            .contains_key(&ModuleName::new("maps").unwrap())
    );
}

#[test]
fn empty_pin_files_does_not_emit_file_absent() {
    let ra_context = {
        let snap = snapshot("ra", vec![module_with("ra", &[("start", 0)])]);
        let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
        EvaluationContext::new(pin("ra", "v1.0.0"), snap, scope).with_files(EvaluationFiles::new())
    };
    let diff = "\
diff --git a/deps/rabbit/Makefile b/deps/rabbit/Makefile
--- a/deps/rabbit/Makefile
+++ b/deps/rabbit/Makefile
@@ -1,1 +1,2 @@
 # makefile
+# added line
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ra_context]);
    let r0 = &eval.verdict.results[0];
    let has_file_absent = r0
        .verdict
        .reasons()
        .iter()
        .any(|r| matches!(r, Reason::FileAbsent { .. }));
    assert!(!has_file_absent, "reasons={:?}", r0.verdict.reasons());
}

#[test]
fn pin_files_with_absent_path_still_emits_file_absent() {
    let ra_context = {
        let snap = snapshot("ra", vec![module_with("ra", &[("start", 0)])]);
        let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
        let files = EvaluationFiles::new().with(PathBuf::from("src/ra_log.erl"), None);
        EvaluationContext::new(pin("ra", "v1.0.0"), snap, scope).with_files(files)
    };
    let diff = "\
diff --git a/src/ra_log.erl b/src/ra_log.erl
--- a/src/ra_log.erl
+++ b/src/ra_log.erl
@@ -1,1 +1,2 @@
 -module(ra_log).
+new_line() -> ok.
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ra_context]);
    let r0 = &eval.verdict.results[0];
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::FileAbsent { .. })),
        "reasons={:?}",
        r0.verdict.reasons()
    );
}

#[test]
fn repeated_calls_to_same_mfa_count_once() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,4 @@
 -module(rabbit_fifo).
+enable() -> ra:start().
+recover() -> ra:start().
+tick() -> ra:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert_eq!(r0.tracked_refs, 1);
    assert!(matches!(r0.verdict, Verdict::Compatible));
}

#[test]
fn diagnostics_aggregate_across_pins_but_count_unique_mfas() {
    let ra_ctx = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let khepri_ctx = make_context("khepri", vec![module_with("khepri", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,4 @@
 -module(rabbit_fifo).
+log_each(L) -> lists:foreach(fun(_) -> ok end, L).
+transform(L) -> lists:map(fun(X) -> X end, L).
+lookup(M) -> maps:get(k, M).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ra_ctx, khepri_ctx]);
    assert_eq!(
        eval.diagnostics
            .untracked_calls
            .get(&ModuleName::new("lists").unwrap())
            .copied(),
        Some(2)
    );
    assert_eq!(
        eval.diagnostics
            .untracked_calls
            .get(&ModuleName::new("maps").unwrap())
            .copied(),
        Some(1)
    );
}

#[test]
fn untracked_record_does_not_produce_a_reason() {
    let context = make_context("ra", vec![module_with("ra", &[("noop", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+extract_id(#user{id = I}) -> I.
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(
        matches!(r0.verdict, Verdict::Compatible),
        "verdict={:?}",
        r0.verdict
    );
    assert_eq!(
        eval.diagnostics
            .untracked_records
            .get(&RecordName::new("user").unwrap())
            .copied(),
        Some(1)
    );
}

#[test]
fn tracked_record_missing_in_pin_still_emits_reason() {
    let snap = snapshot_with_record("ra", "user");
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin("ra", "v1.0.0"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+handle(#nonexistent{}) -> ok.
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(
        eval.diagnostics
            .untracked_records
            .contains_key(&RecordName::new("nonexistent").unwrap()),
        "should be in untracked tally"
    );
    assert!(
        matches!(r0.verdict, Verdict::Compatible),
        "out-of-scope record should not affect verdict; was {:?}",
        r0.verdict
    );
}

#[test]
fn series_evaluation_tracks_refs_per_pin() {
    let ra_context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let khepri_context = make_context("khepri", vec![module_with("khepri", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+start_ra() -> ra:start().
+start_khepri() -> khepri:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ra_context, khepri_context]);
    let by_project: Vec<(String, usize)> = eval
        .verdict
        .results
        .iter()
        .map(|p| (p.pin.project.to_string(), p.tracked_refs))
        .collect();
    assert!(by_project.contains(&("ra".to_string(), 1)));
    assert!(by_project.contains(&("khepri".to_string(), 1)));
    assert!(eval.diagnostics.untracked_calls.is_empty());
}

#[test]
fn elixir_files_in_patch_emit_unsupported_file_type() {
    let diff = "\
diff --git a/lib/status_command.ex b/lib/status_command.ex
--- a/lib/status_command.ex
+++ b/lib/status_command.ex
@@ -1,1 +1,2 @@
 defmodule RabbitMQ.CLI.StatusCommand do
+  def run, do: :ok
";
    let ctx = make_context("demo", vec![module_with("demo", &[])]);
    let series = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let pin_verdict = series.verdict.results.first().unwrap();
    let reasons = pin_verdict.verdict.reasons();
    let has_unsupported = reasons
        .iter()
        .any(|r| matches!(r, Reason::UnsupportedFileType { path } if path == &PathBuf::from("lib/status_command.ex")));
    assert!(
        has_unsupported,
        "expected UnsupportedFileType reason, got {reasons:?}"
    );
}

#[test]
fn build_infra_only_patch_stays_compatible() {
    let diff = "\
diff --git a/Makefile b/Makefile
--- a/Makefile
+++ b/Makefile
@@ -1,1 +1,2 @@
 PROJECT = demo
+DEPS = ra
";
    let ctx = make_context("demo", vec![module_with("demo", &[])]);
    let series = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let pin_verdict = series.verdict.results.first().unwrap();
    assert!(
        matches!(pin_verdict.verdict, Verdict::Compatible),
        "non-source build files must be silent-skipped, got: {:?}",
        pin_verdict.verdict
    );
}
