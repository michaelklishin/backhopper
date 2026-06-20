// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `classify_hunks_against_target`: hunk preimage matching against a
//! file's content on the apply target.

use backhopper_core::compat::classify_hunks_against_target;
use backhopper_core::compat::patch::Patch;
use backhopper_core::model::verdict::Reason;

const MODIFY: &str = "\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -1,3 +1,3 @@
 -module(x).
-old_line.
+new_line.
 tail.
";

const PURE_ADD: &str = "\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+added_line.
";

fn reasons_against(patch: &str, target: &str) -> Vec<Reason> {
    let parsed = Patch::parse(patch.as_bytes()).unwrap();
    let file = &parsed.files[0];
    let path = file.new_path.as_deref().unwrap();
    classify_hunks_against_target(path, &file.hunks, target)
}

#[test]
fn a_clean_preimage_match_yields_no_reason() {
    let reasons = reasons_against(MODIFY, "-module(x).\nold_line.\ntail.\n");
    assert!(reasons.is_empty());
}

#[test]
fn a_diverged_target_yields_preimage_missing() {
    let reasons = reasons_against(MODIFY, "-module(x).\nDIVERGED.\ntail.\n");
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::PreimageMissing { .. })),
        "expected PreimageMissing in {reasons:?}"
    );
}

#[test]
fn a_shifted_preimage_yields_preimage_drifted() {
    let reasons = reasons_against(MODIFY, "%% banner\n-module(x).\nold_line.\ntail.\n");
    match reasons.as_slice() {
        [Reason::PreimageDrifted { line_delta, .. }] => assert_eq!(*line_delta, 1),
        other => panic!("expected one PreimageDrifted, got {other:?}"),
    }
}

// The add/add case: the hunk appends after the last context line where
// the target appended its own divergent line. The preimage matches, yet
// the 3-way merge conflicts.
#[test]
fn a_trailing_add_add_yields_a_collision() {
    let reasons = reasons_against(PURE_ADD, "-module(x).\nDIFFERENT_ADDITION.\n");
    assert!(
        matches!(reasons.as_slice(), [Reason::PostimageCollision { .. }]),
        "expected one PostimageCollision, got {reasons:?}"
    );
}

#[test]
fn a_trailing_append_to_an_unchanged_tail_is_clean() {
    let reasons = reasons_against(PURE_ADD, "-module(x).\n");
    assert!(reasons.is_empty());
}

#[test]
fn an_already_applied_addition_is_clean() {
    let reasons = reasons_against(PURE_ADD, "-module(x).\nadded_line.\n");
    assert!(reasons.is_empty());
}

const LEADING_ADD: &str = "\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -1,1 +1,2 @@
+header_line.
 -module(x).
";

#[test]
fn a_leading_add_add_yields_a_collision() {
    let reasons = reasons_against(LEADING_ADD, "DIFFERENT_HEADER.\n-module(x).\n");
    assert!(
        matches!(reasons.as_slice(), [Reason::PostimageCollision { .. }]),
        "expected one PostimageCollision, got {reasons:?}"
    );
}

#[test]
fn a_leading_add_at_a_clean_top_is_not_flagged() {
    let reasons = reasons_against(LEADING_ADD, "-module(x).\n");
    assert!(reasons.is_empty());
}

const WHOLE_FILE_ADD: &str = "\
diff --git a/src/new.erl b/src/new.erl
new file mode 100644
--- /dev/null
+++ b/src/new.erl
@@ -0,0 +1,2 @@
+-module(new).
+f() -> ok.
";

// A new file the target already created with different content is an
// add/add at the whole-file level.
#[test]
fn a_whole_file_add_over_divergent_target_content_collides() {
    let reasons = reasons_against(WHOLE_FILE_ADD, "-module(new).\nf() -> other.\n");
    assert!(
        matches!(reasons.as_slice(), [Reason::PostimageCollision { .. }]),
        "expected one PostimageCollision, got {reasons:?}"
    );
}

#[test]
fn a_whole_file_add_with_no_target_file_is_clean() {
    let reasons = reasons_against(WHOLE_FILE_ADD, "");
    assert!(reasons.is_empty());
}

const TWO_HUNKS: &str = "\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -1,3 +1,3 @@
 -module(x).
-old_a.
+new_a.
 mid.
@@ -10,3 +10,3 @@
 ctx_b.
-old_b.
+new_b.
 ctx_c.
";

// Per-hunk aggregation: one clean hunk and one diverged hunk in the same
// file still predicts a conflict for the pick.
#[test]
fn one_clean_and_one_diverged_hunk_predicts_a_conflict() {
    let target = "-module(x).\nold_a.\nmid.\n\
                  l4.\nl5.\nl6.\nl7.\nl8.\nl9.\n\
                  ctx_b.\nDIVERGED_B.\nctx_c.\n";
    let reasons = reasons_against(TWO_HUNKS, target);
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::PreimageMissing { .. })),
        "the diverged second hunk must be flagged: {reasons:?}"
    );
}

const EJS_PATCH: &str = "\
diff --git a/priv/www/x.ejs b/priv/www/x.ejs
--- a/priv/www/x.ejs
+++ b/priv/www/x.ejs
@@ -1,3 +1,3 @@
 <html>
-<body>old</body>
+<body>new</body>
 </html>
";

// Preimage matching is language-agnostic: a non-Erlang file whose hunk
// region diverged on target is flagged like any other.
#[test]
fn a_non_erlang_file_is_classified_by_preimage_too() {
    let reasons = reasons_against(EJS_PATCH, "<html>\n<body>DIVERGED</body>\n</html>\n");
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::PreimageMissing { .. })),
        "expected PreimageMissing for the .ejs file: {reasons:?}"
    );
}
