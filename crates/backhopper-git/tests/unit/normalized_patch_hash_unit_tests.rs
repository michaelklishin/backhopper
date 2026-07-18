// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_git::walk::normalized_patch_hash;

#[test]
fn empty_input_has_no_hash() {
    assert_eq!(normalized_patch_hash(b""), None);
    assert_eq!(normalized_patch_hash(b"@@ -1 +1 @@\n"), None);
}

#[test]
fn offset_only_drift_keeps_the_same_hash() {
    let a = b"@@ -1,3 +1,3 @@\n context\n-old\n+new\n";
    let b = b"@@ -41,3 +41,3 @@\n context\n-old\n+new\n";
    assert_eq!(normalized_patch_hash(a), normalized_patch_hash(b));
}

#[test]
fn content_drift_changes_the_hash() {
    let a = b"@@ -1 +1 @@\n-old\n+new\n";
    let b = b"@@ -1 +1 @@\n-old\n+other\n";
    assert_ne!(normalized_patch_hash(a), normalized_patch_hash(b));
}

#[test]
fn distinct_invalid_byte_runs_do_not_collide() {
    // a lossy UTF-8 decode would map both 0xFF and 0xFE to U+FFFD and collide these diffs
    let a = [b'+', 0xFF, b'\n'];
    let b = [b'+', 0xFE, b'\n'];
    assert_ne!(normalized_patch_hash(&a), normalized_patch_hash(&b));
}

#[test]
fn a_trailing_newline_does_not_add_an_empty_line() {
    assert_eq!(
        normalized_patch_hash(b"+line"),
        normalized_patch_hash(b"+line\n")
    );
}
