// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `unified_diff_body` hunk headers must agree with the lines they
//! carry: imara 0.2's bundled printer emitted five context lines under
//! a start that accounted for three, shifting every consumer-derived
//! file line.

use backhopper_git::unified_diff_body;

#[test]
fn an_append_hunk_header_matches_its_context() {
    let before = "-module(backing_queue_SUITE).\n-export([go/0]).\n-spec local_info(state()) -> ok.\nlocal_info(S) -> S.\ngo() -> ok.\n";
    let after = format!("{before}t_info(S) -> a:b(S).\n");
    let body = unified_diff_body(before, &after);
    assert!(
        body.starts_with("@@ -3,3 +3,4 @@\n"),
        "header disagrees with content: {body}"
    );
    // Three context lines, then the addition.
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[1].starts_with(" -spec"));
    assert!(lines[4].starts_with("+t_info"));
}

#[test]
fn a_new_file_hunk_starts_at_zero() {
    let body = unified_diff_body("", "one()\ntwo()\n");
    assert!(body.starts_with("@@ -0,0 +1,2 @@\n"), "got: {body}");
}

#[test]
fn a_mid_file_change_keeps_git_style_context() {
    let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let after = "a\nb\nc\nd\nE\nf\ng\nh\ni\nj\n";
    let body = unified_diff_body(before, after);
    assert!(body.starts_with("@@ -2,7 +2,7 @@\n"), "got: {body}");
    assert!(body.contains("-e\n"));
    assert!(body.contains("+E\n"));
}

#[test]
fn nearby_changes_merge_into_one_hunk() {
    let before = "a\nb\nc\nd\ne\nf\ng\nh\n";
    let after = "a\nB\nc\nd\ne\nf\nG\nh\n";
    let body = unified_diff_body(before, after);
    assert_eq!(
        body.matches("@@").count(),
        2,
        "one hunk, one header pair: {body}"
    );
}

#[test]
fn identical_inputs_yield_an_empty_body() {
    assert!(unified_diff_body("same\n", "same\n").is_empty());
}
