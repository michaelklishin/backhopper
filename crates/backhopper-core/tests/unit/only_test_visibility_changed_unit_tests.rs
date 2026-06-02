// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use time::OffsetDateTime;

use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, Module, Snapshot, SnapshotHeader, Visibility, state,
};
use backhopper_core::model::verdict::{InapplicableReason, SeriesVerdict, TouchedKinds, Verdict};

const VARIANT_A_DIFF: &str = "\
diff --git a/src/rabbit_fifo.erl b/src/rabbit_fifo.erl
--- a/src/rabbit_fifo.erl
+++ b/src/rabbit_fifo.erl
@@ -110,9 +110,8 @@
 some_context_line.
-
--ifdef(TEST).
+%% for tests
 -export([update_header/4,
          chunk_disk_msgs/3]).
--endif.

 more_context_line.
";

const VARIANT_B_DIFF: &str = "\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -1,3 +1,6 @@
 -module(x).
-
--ifdef(TEST).
+%% for tests
 -export([init/1]).
-
--endif.
+
+-spec init([ra:server_id()]) -> state().
+init(Servers) ->
+    ok.
";

const EXPORT_ALL_REWRITE_DIFF: &str = "\
diff --git a/src/y.erl b/src/y.erl
--- a/src/y.erl
+++ b/src/y.erl
@@ -30,5 +30,3 @@
 some_context.
-
--ifdef(TEST).
--compile(export_all).
--endif.
+%% for tests
+-export([helper_a/1, helper_b/2]).
 more_context.
";

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
    }
}

fn empty_pin_context(project: &str) -> EvaluationContext {
    let pin = Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    );
    let m = {
        let mut m = Module::new(ModuleName::new("noop").unwrap());
        m.visibility = Visibility::Public;
        m.exports.push(FunArity {
            name: FunctionName::new("noop").unwrap(),
            arity: Arity::new(0),
        });
        m
    };
    let snap: Snapshot<state::Canonical> =
        Snapshot::from_extracted(header(project), vec![m], vec![]).into_canonical();
    let scope = PinScope::from_snapshot(ProjectName::new(project).unwrap(), &snap, []);
    EvaluationContext::new(pin, snap, scope)
}

fn touched_paths_in_diff(diff: &str) -> Vec<PathBuf> {
    Patch::parse(diff.as_bytes())
        .unwrap()
        .files
        .iter()
        .filter_map(|f| f.new_path.clone().or_else(|| f.old_path.clone()))
        .collect()
}

#[test]
fn inapplicable_reason_returns_visibility_changed_when_flag_set() {
    let tk = TouchedKinds {
        only_test_visibility: true,
        erl: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlyTestVisibilityChanged)
    );
}

#[test]
fn visibility_flag_dominates_other_kinds() {
    let tk = TouchedKinds {
        only_test_visibility: true,
        erl: 2,
        docs: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlyTestVisibilityChanged)
    );
}

#[test]
fn unset_visibility_flag_falls_through_to_existing_rules() {
    let tk = TouchedKinds {
        erl: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(tk.inapplicable_reason(), None);
}

#[test]
fn variant_a_diff_is_only_test_visibility() {
    let analyzed = Patch::parse(VARIANT_A_DIFF.as_bytes()).unwrap().analyze();
    assert!(
        analyzed.is_only_test_visibility_change(),
        "Variant A unwrap should classify as visibility-only"
    );
}

#[test]
fn variant_b_diff_is_not_only_test_visibility() {
    let analyzed = Patch::parse(VARIANT_B_DIFF.as_bytes()).unwrap().analyze();
    assert!(
        !analyzed.is_only_test_visibility_change(),
        "Variant B unwrap exposes a -spec and body — must fall through to normal analyzer"
    );
}

#[test]
fn export_all_bracket_form_rewrite_is_only_test_visibility() {
    let diff = "\
diff --git a/src/y.erl b/src/y.erl
--- a/src/y.erl
+++ b/src/y.erl
@@ -10,5 +10,3 @@
 context.
-
--ifdef(TEST).
--compile([export_all]).
--endif.
+%% for tests
+-export([helper_a/1]).
 more.
";
    let analyzed = Patch::parse(diff.as_bytes()).unwrap().analyze();
    assert!(analyzed.is_only_test_visibility_change());
}

#[test]
fn export_all_rewrite_is_only_test_visibility() {
    let analyzed = Patch::parse(EXPORT_ALL_REWRITE_DIFF.as_bytes())
        .unwrap()
        .analyze();
    assert!(
        analyzed.is_only_test_visibility_change(),
        "-compile(export_all) → explicit -export rewrite should be visibility-only"
    );
}

#[test]
fn empty_patch_is_not_only_test_visibility() {
    let analyzed = Patch::parse(b"").unwrap().analyze();
    assert!(!analyzed.is_only_test_visibility_change());
}

#[test]
fn erl_visibility_with_non_erl_change_alongside_still_visibility() {
    // A docs touch coexists with a Variant A unwrap. Non-Erlang
    // hunks don't disqualify the classification: the contract is
    // that every Erlang hunk is visibility-only.
    let mixed = "\
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,1 +1,1 @@
-old
+new
diff --git a/src/rabbit_fifo.erl b/src/rabbit_fifo.erl
--- a/src/rabbit_fifo.erl
+++ b/src/rabbit_fifo.erl
@@ -110,5 +110,4 @@
 context.
--ifdef(TEST).
+%% for tests
 -export([helper/0]).
--endif.
";
    let analyzed = Patch::parse(mixed.as_bytes()).unwrap().analyze();
    assert!(analyzed.is_only_test_visibility_change());
}

#[test]
fn two_erl_files_one_visibility_one_real_is_not_visibility() {
    let mixed = "\
diff --git a/src/a.erl b/src/a.erl
--- a/src/a.erl
+++ b/src/a.erl
@@ -1,3 +1,2 @@
 context.
--ifdef(TEST).
+%% for tests
 -export([helper/0]).
--endif.
diff --git a/src/b.erl b/src/b.erl
--- a/src/b.erl
+++ b/src/b.erl
@@ -1,1 +1,2 @@
 -module(b).
+go() -> ok.
";
    let analyzed = Patch::parse(mixed.as_bytes()).unwrap().analyze();
    assert!(!analyzed.is_only_test_visibility_change());
}

#[test]
fn variant_a_promotes_to_inapplicable_with_visibility_reason() {
    let analyzed = Patch::parse(VARIANT_A_DIFF.as_bytes()).unwrap().analyze();
    let visibility = analyzed.is_only_test_visibility_change();
    let touched_paths = touched_paths_in_diff(VARIANT_A_DIFF);
    let mut touched = TouchedKinds::from_paths(&touched_paths);
    touched.only_test_visibility = visibility;
    let eval = analyzed.evaluate_series(&[empty_pin_context("ra")]);
    let stamped: Vec<_> = eval
        .verdict
        .results
        .into_iter()
        .map(|pv| pv.with_touched(touched))
        .collect();
    let promoted = SeriesVerdict::from_results(stamped).promote_inapplicable();
    let r0 = &promoted.results[0];
    assert!(
        matches!(
            r0.verdict,
            Verdict::Inapplicable {
                reason: InapplicableReason::OnlyTestVisibilityChanged
            }
        ),
        "verdict was {:?}",
        r0.verdict
    );
}

#[test]
fn variant_b_falls_through_to_normal_analyzer_not_inapplicable() {
    let analyzed = Patch::parse(VARIANT_B_DIFF.as_bytes()).unwrap().analyze();
    let visibility = analyzed.is_only_test_visibility_change();
    assert!(!visibility);
    let touched_paths = touched_paths_in_diff(VARIANT_B_DIFF);
    let mut touched = TouchedKinds::from_paths(&touched_paths);
    touched.only_test_visibility = visibility;
    let eval = analyzed.evaluate_series(&[empty_pin_context("ra")]);
    let stamped: Vec<_> = eval
        .verdict
        .results
        .into_iter()
        .map(|pv| pv.with_touched(touched))
        .collect();
    let promoted = SeriesVerdict::from_results(stamped).promote_inapplicable();
    let r0 = &promoted.results[0];
    // With ra not in scope, the type ref to ra:server_id/0 is filtered;
    // verdict ends up Compatible, not Inapplicable.
    assert!(
        !matches!(r0.verdict, Verdict::Inapplicable { .. }),
        "Variant B must not promote to Inapplicable, got {:?}",
        r0.verdict
    );
}
