// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::verdict::{ContentPresence, Reason, SeriesEvaluation};
use backhopper_test_support::{canonical_snapshot, module, snapshot_header};

fn pin() -> Pin {
    Pin::new(
        ProjectName::new("demo").unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

fn snapshot() -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("demo", "v1.0.0"), vec![module("demo")])
}

fn evaluate(patch: &str, pin_file: &[u8]) -> SeriesEvaluation {
    let snap = snapshot();
    let scope = PinScope::from_snapshot(ProjectName::new("demo").unwrap(), &snap, Vec::new());
    let files = EvaluationFiles::new().with(PathBuf::from("src/demo.erl"), Some(pin_file.to_vec()));
    let ctx = EvaluationContext::new(pin(), snap, scope).with_files(files);
    Patch::parse(patch.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx])
}

fn presence(patch: &str, pin_file: &[u8]) -> Option<ContentPresence> {
    evaluate(patch, pin_file)
        .diagnostics
        .already_present
        .and_then(|ap| ap.content)
}

// A modification hunk: context above, one line replaced.
const MODIFICATION: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,3 +1,3 @@
 -module(demo).
-greet(Name) -> Name.
+greet(Name) -> {ok, Name}.
 farewell() -> ok.
";

#[test]
fn modification_already_on_target_counts_as_applied() {
    // The pin file already carries the post-image.
    let target = b"-module(demo).\ngreet(Name) -> {ok, Name}.\nfarewell() -> ok.\n";
    let p = presence(MODIFICATION, target).expect("tally expected");
    assert_eq!(p.hunks_considered, 1);
    assert_eq!(p.hunks_already_applied, 1);
    assert_eq!(p.hunks_ambiguous, 0);
    assert!(p.fully_present());
    assert_eq!(p.pin.as_str(), "demo");
    let file_tally = &p.per_file[&"src/demo.erl".parse().unwrap()];
    assert_eq!(file_tally.applied, 1);
}

#[test]
fn modification_not_yet_applied_counts_zero_applied() {
    // The pin file is still the pre-image.
    let target = b"-module(demo).\ngreet(Name) -> Name.\nfarewell() -> ok.\n";
    let p = presence(MODIFICATION, target).expect("tally expected");
    assert_eq!(p.hunks_considered, 1);
    assert_eq!(p.hunks_already_applied, 0);
    assert!(!p.fully_present());
}

// A pure addition with context on both sides: the inserted line
// splits the context block, so pre- and post-image cannot both match.
const ADDITION_BETWEEN_CONTEXT: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,2 +1,3 @@
 -module(demo).
+added_fun() -> ok.
 farewell() -> ok.
";

#[test]
fn addition_already_present_between_context_counts_as_applied() {
    let target = b"-module(demo).\nadded_fun() -> ok.\nfarewell() -> ok.\n";
    let p = presence(ADDITION_BETWEEN_CONTEXT, target).expect("tally expected");
    assert_eq!(p.hunks_already_applied, 1);
}

#[test]
fn addition_absent_from_target_is_not_applied() {
    let target = b"-module(demo).\nfarewell() -> ok.\n";
    let p = presence(ADDITION_BETWEEN_CONTEXT, target).expect("tally expected");
    assert_eq!(p.hunks_already_applied, 0);
}

// Trailing addition after a single context line: when the addition
// is present, the lone context line still matches contiguously on
// its own, so the hunk is genuinely undecidable.
const TRAILING_ADDITION: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,1 +1,2 @@
 -module(demo).
+added_fun() -> ok.
";

#[test]
fn trailing_addition_with_surviving_context_is_ambiguous() {
    let target = b"-module(demo).\nadded_fun() -> ok.\n";
    let p = presence(TRAILING_ADDITION, target).expect("tally expected");
    assert_eq!(p.hunks_ambiguous, 1);
    assert_eq!(p.hunks_already_applied, 0);
}

const DELETION_ONLY: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,2 +1,1 @@
 -module(demo).
-greet(Name) -> Name.
";

#[test]
fn deletion_only_patch_gets_no_content_signal() {
    let target = b"-module(demo).\n";
    assert!(presence(DELETION_ONLY, target).is_none());
}

// An added file whose content already exists at the pin: the empty
// preimage classifies `Exact`, so the postimage alone must decide.
const ADDED_FILE: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
new file mode 100644
--- /dev/null
+++ b/src/demo.erl
@@ -0,0 +1,2 @@
+-module(demo).
+greet(Name) -> Name.
";

#[test]
fn added_file_already_at_pin_counts_as_applied() {
    let target = b"-module(demo).\ngreet(Name) -> Name.\n";
    let p = presence(ADDED_FILE, target).expect("tally expected");
    assert_eq!(p.hunks_already_applied, 1);
}

// A context-less single-line addition is too weak a needle to call
// applied; it lands in the low-confidence bucket instead.
const ONE_LINE_NEW_FILE: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
new file mode 100644
--- /dev/null
+++ b/src/demo.erl
@@ -0,0 +1,1 @@
+-module(demo).
";

#[test]
fn context_less_one_liner_is_low_confidence() {
    let target = b"-module(demo).\n";
    let p = presence(ONE_LINE_NEW_FILE, target).expect("tally expected");
    assert_eq!(p.hunks_low_confidence, 1);
    assert_eq!(p.hunks_already_applied, 0);
}

#[test]
fn non_utf8_pin_file_is_skipped_without_a_tally() {
    let target: &[u8] = &[0xff, 0xfe, 0x00, 0x01];
    assert!(presence(MODIFICATION, target).is_none());
}

#[test]
fn tally_is_a_diagnostic_and_preimage_reasons_still_fire() {
    // Already-applied content drifts the preimage: the existing
    // PreimageMissing reason and the new tally coexist, and the
    // verdict keeps coming from reasons alone.
    let applied = b"-module(demo).\ngreet(Name) -> {ok, Name}.\nfarewell() -> ok.\n";
    let evaluation = evaluate(MODIFICATION, applied);
    let reasons = evaluation.verdict.results[0].verdict.reasons();
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::PreimageMissing { .. })),
        "expected PreimageMissing alongside the tally, got {reasons:?}"
    );
    assert!(evaluation.diagnostics.already_present.is_some());
}

// Two hunks, one already applied and one not: the tally reports the
// fraction instead of rounding to a boolean.
const TWO_HUNK_PATCH: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,3 +1,3 @@
 -module(demo).
-greet(Name) -> Name.
+greet(Name) -> {ok, Name}.
 middle() -> ok.
@@ -5,3 +5,3 @@
 spacer() -> ok.
-farewell() -> ok.
+farewell() -> {ok, bye}.
 ending() -> ok.
";

#[test]
fn partially_applied_patch_reports_the_fraction() {
    // First hunk's post-image present, second hunk still pre-image.
    let target = b"-module(demo).\ngreet(Name) -> {ok, Name}.\nmiddle() -> ok.\npad() -> ok.\nspacer() -> ok.\nfarewell() -> ok.\nending() -> ok.\n";
    let p = presence(TWO_HUNK_PATCH, target).expect("tally expected");
    assert_eq!(p.hunks_considered, 2);
    assert_eq!(p.hunks_already_applied, 1);
    assert!(!p.fully_present());
}
