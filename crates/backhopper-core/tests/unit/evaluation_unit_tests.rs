// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use time::OffsetDateTime;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, HrlFile, Module, RecordDecl, RecordField, Snapshot, SnapshotHeader, SpecSig,
    Visibility, state,
};
use backhopper_core::model::verdict::{Reason, Verdict};

fn header(project: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

fn module_with(name: &str, exports: &[(&str, u8)]) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.visibility = Visibility::Public;
    for (f, a) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(*f).unwrap(),
            arity: Arity::new(*a),
        });
    }
    m
}

fn pin_for(project: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

fn snapshot(project: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header(project), modules, vec![]).into_canonical()
}

fn snapshot_with_record(project: &str, record: &str) -> Snapshot<state::Canonical> {
    let mut hrl = HrlFile::new(format!("include/{project}.hrl"));
    hrl.records.push(RecordDecl {
        name: RecordName::new(record).unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("id").unwrap(),
            type_repr: None,
        }],
    });
    Snapshot::from_extracted(header(project), vec![], vec![hrl]).into_canonical()
}

fn make_context(project: &str, modules_in_snapshot: Vec<Module>) -> EvaluationContext {
    let pin = pin_for(project);
    let snap = snapshot(project, modules_in_snapshot);
    let scope = PinScope::from_snapshot(ProjectName::new(project).unwrap(), &snap, []);
    EvaluationContext::new(pin, snap, scope)
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
        let pin = pin_for("ra");
        let snap = snapshot("ra", vec![module_with("ra", &[("start", 0)])]);
        let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
        EvaluationContext::new(pin, snap, scope).with_files(EvaluationFiles::new())
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
        let pin = pin_for("ra");
        let snap = snapshot("ra", vec![module_with("ra", &[("start", 0)])]);
        let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
        let files = EvaluationFiles::new().with(PathBuf::from("src/ra_log.erl"), None);
        EvaluationContext::new(pin, snap, scope).with_files(files)
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
    let pin = pin_for("ra");
    let snap = snapshot_with_record("ra", "user");
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin, snap, scope);
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
fn apply_3_callback_dispatch_does_not_break_verdict() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+init(#{module := Mod, args := Args}) ->
+    apply(Mod, init, [Args]).
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
    assert!(
        eval.diagnostics.unanalyzed.apply >= 1,
        "apply should be tallied: {:?}",
        eval.diagnostics.unanalyzed
    );
}

#[test]
fn variable_module_dispatch_does_not_break_verdict() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+dispatch(#{handler := Mod, state := State}, Cmd) ->
+    Mod:handle_command(Cmd, State).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(matches!(r0.verdict, Verdict::Compatible));
    assert!(
        eval.diagnostics.unanalyzed.variable_dispatch >= 1,
        "Mod:fun should be tallied: {:?}",
        eval.diagnostics.unanalyzed
    );
}

#[test]
fn spawn_family_counts_as_apply() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+spin_up(Mod, Args) ->
+    spawn_link(Mod, init, [Args]).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    assert!(eval.diagnostics.unanalyzed.apply >= 1);
}

#[test]
fn mixed_static_and_dynamic_keeps_them_separate() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,5 @@
 -module(rabbit_fifo).
+start_child(Mod, Args) ->
+    ok = ra:start(),
+    Mod:bootstrap(Args),
+    apply(Mod, terminate, [shutdown]).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let r0 = &eval.verdict.results[0];
    assert!(matches!(r0.verdict, Verdict::Compatible));
    assert_eq!(r0.tracked_refs, 1);
    assert!(eval.diagnostics.unanalyzed.apply >= 1);
    assert!(eval.diagnostics.unanalyzed.variable_dispatch >= 1);
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
    let snap = snapshot("demo", vec![module_with("demo", &[])]);
    let verdict = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against(&snap, pin_for("demo"));
    let pin_verdict = verdict.verdicts().first().unwrap();
    let reasons = pin_verdict.verdict.reasons();
    let has_unsupported = reasons
        .iter()
        .any(|r| matches!(r, Reason::UnsupportedFileType { path } if path == &PathBuf::from("lib/status_command.ex")));
    assert!(
        has_unsupported,
        "expected UnsupportedFileType reason, got {reasons:?}"
    );
}

fn snapshot_with_module_specs(
    project: &str,
    module_name: &str,
    specs: Vec<(&str, u8, &str)>,
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new(module_name).unwrap());
    m.visibility = Visibility::Public;
    for (n, a, _) in &specs {
        m.exports.push(FunArity {
            name: FunctionName::new(*n).unwrap(),
            arity: Arity::new(*a),
        });
    }
    for (n, a, sig) in &specs {
        m.specs.push(SpecSig {
            name: FunctionName::new(*n).unwrap(),
            arity: Arity::new(*a),
            signature: (*sig).to_string(),
        });
    }
    snapshot(project, vec![m])
}

fn snapshot_with_record_fields(
    project: &str,
    record_name: &str,
    fields: Vec<&str>,
) -> Snapshot<state::Canonical> {
    let mut hrl = HrlFile::new(format!("include/{project}.hrl"));
    hrl.records.push(RecordDecl {
        name: RecordName::new(record_name).unwrap(),
        fields: fields
            .into_iter()
            .map(|n| RecordField {
                name: FieldName::new(n).unwrap(),
                type_repr: None,
            })
            .collect(),
    });
    let m = Module::new(ModuleName::new("demo").unwrap());
    Snapshot::from_extracted(header(project), vec![m], vec![hrl]).into_canonical()
}

#[test]
fn signature_changed_fires_when_source_spec_differs_from_target() {
    let target =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), atom()) -> ok")]);
    let source =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), term()) -> ok")]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+build(R) -> ra:new(R, default).
";
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin_for("ra"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_sig_changed = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::SignatureChanged { module, function, arity, .. }
                if module.as_str() == "ra"
                    && function.as_str() == "new"
                    && *arity == Arity::new(2)
        )
    });
    assert!(
        has_sig_changed,
        "expected SignatureChanged reason, got {reasons:?}"
    );
}

#[test]
fn signature_changed_silent_when_specs_match() {
    let target =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), term()) -> ok")]);
    let source =
        snapshot_with_module_specs("ra", "ra", vec![("new", 2, "new(atom(), term()) -> ok")]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+build(R) -> ra:new(R, default).
";
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin_for("ra"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Compatible
    ));
}

#[test]
fn record_fields_changed_fires_when_source_record_has_extra_fields() {
    let target = snapshot_with_record_fields("demo", "user", vec!["id", "name"]);
    let source = snapshot_with_record_fields("demo", "user", vec!["id", "name", "email"]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+email_of(U) -> U#user.email.
";
    let scope = PinScope::from_snapshot(ProjectName::new("demo").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin_for("demo"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_record_changed = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::RecordFieldsChanged { record, expected, found }
                if record.as_str() == "user"
                    && expected.len() == 3
                    && found.len() == 2
        )
    });
    assert!(
        has_record_changed,
        "expected RecordFieldsChanged reason, got {reasons:?}"
    );
}

fn snapshot_with_inline_record_fields(
    project: &str,
    record_name: &str,
    fields: Vec<&str>,
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new("demo").unwrap());
    m.records.push(RecordDecl {
        name: RecordName::new(record_name).unwrap(),
        fields: fields
            .into_iter()
            .map(|n| RecordField {
                name: FieldName::new(n).unwrap(),
                type_repr: None,
            })
            .collect(),
    });
    Snapshot::from_extracted(header(project), vec![m], vec![]).into_canonical()
}

// Records declared inline in a module (`.erl`), not only in headers (`.hrl`),
// must be resolved so field changes are still detected.
#[test]
fn record_fields_changed_fires_for_inline_module_record() {
    let target = snapshot_with_inline_record_fields("demo", "user", vec!["id", "name"]);
    let source = snapshot_with_inline_record_fields("demo", "user", vec!["id", "name", "email"]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+email_of(U) -> U#user.email.
";
    let scope = PinScope::from_snapshot(ProjectName::new("demo").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin_for("demo"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_record_changed = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::RecordFieldsChanged { record, expected, found }
                if record.as_str() == "user"
                    && expected.len() == 3
                    && found.len() == 2
        )
    });
    assert!(
        has_record_changed,
        "expected RecordFieldsChanged reason, got {reasons:?}"
    );
}

fn snapshot_with_clauses(
    project: &str,
    module_name: &str,
    function_name: &str,
    arity: u8,
    clauses: Vec<Vec<ArgShape>>,
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new(module_name).unwrap());
    m.visibility = Visibility::Public;
    let fa = FunArity {
        name: FunctionName::new(function_name).unwrap(),
        arity: Arity::new(arity),
    };
    m.exports.push(fa.clone());
    m.clause_heads.insert(fa, clauses);
    snapshot(project, vec![m])
}

#[test]
fn clause_mismatch_fires_when_args_dont_satisfy_clause_head() {
    let snap = snapshot_with_clauses(
        "ra",
        "ra",
        "mode",
        1,
        vec![
            vec![ArgShape::Atom {
                name: "start".into(),
            }],
            vec![ArgShape::Atom {
                name: "stop".into(),
            }],
        ],
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_clause_mismatch = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::ClauseMismatch { module, function, arity, .. }
                if module.as_str() == "ra"
                    && function.as_str() == "mode"
                    && *arity == Arity::new(1)
        )
    });
    assert!(
        has_clause_mismatch,
        "expected ClauseMismatch reason, got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_silent_when_clauses_empty() {
    let context = make_context("ra", vec![module_with("ra", &[("mode", 1)])]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::ClauseMismatch { .. })),
        "no ClauseMismatch when clause_heads is empty, got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_fires_when_one_of_many_calls_fails_match() {
    let snap = snapshot_with_clauses(
        "ra",
        "ra",
        "mode",
        1,
        vec![vec![ArgShape::Atom {
            name: "start".into(),
        }]],
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,3 @@
 -module(rabbit_fifo).
+ok_call() -> ra:mode(start).
+bad_call() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_clause_mismatch = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::ClauseMismatch { call_args, .. }
                if call_args.iter().any(|a| matches!(a, ArgShape::Atom { name } if name == "restart"))
        )
    });
    assert!(
        has_clause_mismatch,
        "ClauseMismatch must surface the failing call (restart), got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_silent_when_call_arg_is_variable() {
    let snap = snapshot_with_clauses(
        "ra",
        "ra",
        "mode",
        1,
        vec![vec![ArgShape::Atom {
            name: "start".into(),
        }]],
    );
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run(Cmd) -> ra:mode(Cmd).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::ClauseMismatch { .. })),
        "variable call args are unknown ground; must not flag, got {reasons:?}"
    );
}

#[test]
fn clause_mismatch_suppressed_when_signature_changed_already_fired() {
    let make_module = |spec: &str| {
        let mut m = Module::new(ModuleName::new("ra").unwrap());
        m.visibility = Visibility::Public;
        let fa = FunArity {
            name: FunctionName::new("mode").unwrap(),
            arity: Arity::new(1),
        };
        m.exports.push(fa.clone());
        m.specs.push(SpecSig {
            name: FunctionName::new("mode").unwrap(),
            arity: Arity::new(1),
            signature: spec.into(),
        });
        m.clause_heads.insert(
            fa,
            vec![vec![ArgShape::Atom {
                name: "start".into(),
            }]],
        );
        m
    };
    let target = snapshot("ra", vec![make_module("mode(atom()) -> ok")]);
    let source = snapshot("ra", vec![make_module("mode(term()) -> ok")]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin_for("ra"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+run() -> ra:mode(restart).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_signature_changed = reasons
        .iter()
        .any(|r| matches!(r, Reason::SignatureChanged { .. }));
    let has_clause_mismatch = reasons
        .iter()
        .any(|r| matches!(r, Reason::ClauseMismatch { .. }));
    assert!(
        has_signature_changed,
        "SignatureChanged must fire first, got {reasons:?}"
    );
    assert!(
        !has_clause_mismatch,
        "ClauseMismatch must be suppressed when SignatureChanged is already present, got {reasons:?}"
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
    let snap = snapshot("demo", vec![module_with("demo", &[])]);
    let verdict = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .against(&snap, pin_for("demo"));
    let pin_verdict = verdict.verdicts().first().unwrap();
    assert!(
        matches!(pin_verdict.verdict, Verdict::Compatible),
        "non-source build files must be silent-skipped, got: {:?}",
        pin_verdict.verdict
    );
}
