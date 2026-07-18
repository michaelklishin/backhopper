// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_qualified_calls`: statically-named `mod:fun(Args)` calls
//! with line and arity, excluding type references and dynamic dispatch.

use backhopper_core::compat::call_sites::extract_qualified_calls;

// A contiguous file: text line i is file line i.
fn seq_map(src: &str) -> Vec<u32> {
    (1..=src.lines().count() as u32).collect()
}

fn calls(src: &str) -> Vec<(String, String, u8, u32)> {
    extract_qualified_calls(src, &seq_map(src))
        .into_iter()
        .map(|c| {
            (
                c.mfa.module.to_string(),
                c.mfa.function.to_string(),
                c.mfa.arity.get(),
                c.line,
            )
        })
        .collect()
}

#[test]
fn a_qualified_call_is_captured_with_line_and_arity() {
    let got = calls("f(V, Q) -> rabbit_misc:queue_resource(V, Q).\n");
    assert_eq!(
        got,
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn the_line_is_the_source_line() {
    let got = calls("a() -> ok.\nb() -> ok.\nf() -> m:g(x).\n");
    assert_eq!(got, [("m".to_owned(), "g".to_owned(), 1, 3)]);
}

// A qualified type in a -spec is a type reference, not a call.
#[test]
fn a_qualified_type_in_a_spec_is_skipped() {
    let got = calls("-spec f(othermod:t()) -> ok.\nf(_) -> ok.\n");
    assert!(got.is_empty(), "unexpected: {got:?}");
}

// A multi-line -spec stays a type-attribute region across lines.
#[test]
fn a_multi_line_spec_is_skipped() {
    let got = calls("-spec f(X) -> ok when\n      X :: othermod:t().\nf(_) -> ok.\n");
    assert!(got.is_empty(), "unexpected: {got:?}");
}

// A variable-module call is dynamic dispatch, not a statically-named call.
#[test]
fn a_variable_module_call_is_skipped() {
    let got = calls("f(Mod) -> Mod:go(x).\n");
    assert!(got.is_empty(), "unexpected: {got:?}");
}

// Joined runs

#[test]
fn a_wrapped_call_recovers_its_exact_arity() {
    let got = calls(
        "f(V, Q) -> rabbit_misc:queue_resource(V,\n                                       Q).\n",
    );
    assert_eq!(
        got,
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn a_wrapped_call_reports_the_line_its_match_starts_on() {
    let src = "f(S) -> ok,\n    rabbit_classic_queue_index_v2:info(S,\n        extended).\n";
    let got = calls(src);
    assert_eq!(
        got,
        [(
            "rabbit_classic_queue_index_v2".to_owned(),
            "info".to_owned(),
            2,
            2
        )]
    );
}

#[test]
fn non_adjacent_lines_do_not_join() {
    // Two hunks at file lines 10 and 50: joined they would form one call; apart, neither holds a call.
    let src = "f(V) -> rabbit_misc:queue_resource(V,\n    Q).\n";
    let got = extract_qualified_calls(src, &[10, 50]);
    assert!(got.is_empty(), "unexpected: {got:?}");
}

#[test]
fn a_spec_boundary_splits_the_run() {
    // The wrapped -spec is type context; the call after it is body, attributed to its own line.
    let src = "-spec info(state()) ->\n    list().\nf(S) -> rabbit_misc:format(S, []).\n";
    let got = calls(src);
    assert_eq!(got, [("rabbit_misc".to_owned(), "format".to_owned(), 2, 3)]);
}

#[test]
fn a_qualified_type_after_the_spec_opens_is_still_skipped() {
    // Classification persists across a gap: the -spec opened at line 3 is still open at line 90.
    let src = "-spec f(othermod:t(),\nothermod:u()) -> ok.\n";
    let got = extract_qualified_calls(src, &[3, 90]);
    assert!(got.is_empty(), "unexpected: {got:?}");
}

#[test]
fn repro_e2e_three_line_run_attribution() {
    let src = "t_info(S) -> rabbit_classic_queue_index_v2:info(S).\nt_segment(Seg, Dir) -> rabbit_classic_queue_index_v2:segment_file(Seg, Dir).\nt_local(S) -> local_info(S).\n";
    let got = extract_qualified_calls(src, &[6, 7, 8]);
    let lines: Vec<(String, u32)> = got
        .iter()
        .map(|c| (c.mfa.function.to_string(), c.line))
        .collect();
    assert_eq!(
        lines,
        [("info".to_owned(), 1), ("segment_file".to_owned(), 2)]
    );
}
