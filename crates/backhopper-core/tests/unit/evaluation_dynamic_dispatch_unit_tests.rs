// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Dynamic dispatch handling: `apply/3`, the spawn family, and
//! variable-module calls are tallied as unanalyzed, never as reasons.

use backhopper_core::compat::patch::Patch;
use backhopper_core::model::verdict::Verdict;
use backhopper_test_support::module_with;

use crate::evaluation_support::make_context;

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
