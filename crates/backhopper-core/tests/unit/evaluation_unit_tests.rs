use std::path::PathBuf;

use time::OffsetDateTime;

use backhopper_core::compat::patch::{Language, Patch, PinContext, PinFiles};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, HrlFile, Module, RecordDecl, RecordField, Snapshot, SnapshotHeader, Visibility, state,
};
use backhopper_core::model::verdict::{Reason, Verdict};

fn header(project: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
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
    let mut hrl = HrlFile::new(format!("include/{}.hrl", project));
    hrl.records.push(RecordDecl {
        name: RecordName::new(record).unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("id").unwrap(),
            type_repr: None,
        }],
    });
    Snapshot::from_extracted(header(project), vec![], vec![hrl]).into_canonical()
}

fn make_context(project: &str, modules_in_snapshot: Vec<Module>) -> PinContext {
    let pin = pin_for(project);
    let snap = snapshot(project, modules_in_snapshot);
    let scope = PinScope::from_snapshot(ProjectName::new(project).unwrap(), &snap, []);
    PinContext::new(pin, snap, scope)
}

#[test]
fn out_of_scope_module_call_does_not_produce_a_reason() {
    let context = make_context("ra", vec![module_with("ra", &[("noop", 0)])]);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> lists:foreach(fun(_) -> ok end, []).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:nonexistent_function(1, 2, 3).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,3 @@
 -module(x).
+go(L) -> lists:foreach(fun(_) -> ok end, L).
+gor(M) -> maps:get(k, M, default).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
        PinContext::new(pin, snap, scope).with_files(PinFiles::new())
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
        .analyze(Language::Erlang)
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
        let files = PinFiles::new().with(PathBuf::from("src/ra_log.erl"), None);
        PinContext::new(pin, snap, scope).with_files(files)
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
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,4 @@
 -module(x).
+a() -> ra:start().
+b() -> ra:start().
+c() -> ra:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,4 @@
 -module(x).
+a(L) -> lists:foreach(fun(_) -> ok end, L).
+b(L) -> lists:map(fun(X) -> X end, L).
+c(M) -> maps:get(k, M).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go(#user{id = I}) -> I.
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
    let context = PinContext::new(pin, snap, scope);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go(#nonexistent{}) -> ok.
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,3 @@
 -module(x).
+init(#{module := Mod, args := Args}) ->
+    apply(Mod, init, [Args]).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,3 @@
 -module(x).
+dispatch(#{handler := Mod, state := State}, Cmd) ->
+    Mod:handle_command(Cmd, State).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,3 @@
 -module(x).
+spin_up(Mod, Args) ->
+    spawn_link(Mod, init, [Args]).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
        .evaluate_series(&[context]);
    assert!(eval.diagnostics.unanalyzed.apply >= 1);
}

#[test]
fn mixed_static_and_dynamic_keeps_them_separate() {
    let context = make_context("ra", vec![module_with("ra", &[("start", 0)])]);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,5 @@
 -module(x).
+go(Mod, Args) ->
+    ok = ra:start(),
+    Mod:bootstrap(Args),
+    apply(Mod, terminate, [shutdown]).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,3 @@
 -module(x).
+a() -> ra:start().
+b() -> khepri:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze(Language::Erlang)
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
