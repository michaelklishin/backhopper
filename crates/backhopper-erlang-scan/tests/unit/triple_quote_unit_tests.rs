// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! EEP-64 triple-quoted strings: the span primitive and the argument
//! scanners that consult it.

use backhopper_erlang_scan::{
    ScanArity, count_top_level_commas, scan_arity, split_top_level_args, split_top_level_commas,
    triple_quoted_span,
};

fn span_of(src: &str) -> Option<&str> {
    triple_quoted_span(src.as_bytes(), 0).map(|n| &src[..n])
}

#[test]
fn three_quotes_open_a_string_that_ends_at_its_closing_line() {
    let src = "\"\"\"\nFunctions for public-key infrastructure.\n\"\"\"";
    assert_eq!(span_of(src), Some(src));
}

#[test]
fn a_closer_needs_at_least_as_many_quotes_as_the_opener() {
    let src = "\"\"\"\"\ntext\n\"\"\"\nstill inside\n\"\"\"\"\nafter";
    let span = span_of(src).unwrap();
    assert!(span.ends_with("\"\"\"\""));
    assert!(span.contains("still inside"));
}

#[test]
fn a_longer_closing_run_still_closes() {
    let src = "\"\"\"\ntext\n\"\"\"\"\"\nafter";
    assert_eq!(span_of(src), Some("\"\"\"\ntext\n\"\"\"\"\""));
}

#[test]
fn an_indented_closer_closes() {
    let src = "\"\"\"\n  text\n    \"\"\"\nafter";
    assert_eq!(span_of(src), Some("\"\"\"\n  text\n    \"\"\""));
}

// the attribute's terminating dot follows the closing run on the same
// line, and stays outside the span for the caller to find
#[test]
fn the_span_ends_at_the_closing_run_not_the_terminating_dot() {
    let src = "\"\"\"\ntext\n\"\"\".\n-export([start/0]).";
    assert_eq!(span_of(src), Some("\"\"\"\ntext\n\"\"\""));
}

#[test]
fn an_unterminated_opener_runs_to_the_end_of_the_input() {
    let src = "\"\"\"\ntext with no closer\n";
    assert_eq!(span_of(src), Some(src));
}

#[test]
fn one_and_two_quotes_are_ordinary_strings() {
    assert_eq!(span_of("\"text\""), None);
    assert_eq!(span_of("\"\""), None);
}

#[test]
fn a_position_that_is_not_a_quote_has_no_span() {
    assert_eq!(triple_quoted_span(b"-export([start/0]).", 0), None);
    assert_eq!(triple_quoted_span(b"", 0), None);
    assert_eq!(triple_quoted_span(b"abc", 9), None);
}

#[test]
fn commas_inside_a_triple_quoted_argument_do_not_raise_the_arity() {
    let args = "Config, \"\"\"\none, two, three\n\"\"\", Timeout)";
    assert_eq!(scan_arity(args), ScanArity::Exact(3));
}

#[test]
fn an_end_keyword_inside_a_triple_quoted_argument_does_not_desync_block_depth() {
    let args = "\"\"\"\ncase X of _ -> ok end\n\"\"\", Timeout)";
    assert_eq!(scan_arity(args), ScanArity::Exact(2));
}

#[test]
fn an_unbalanced_quote_inside_a_triple_quoted_argument_stays_content() {
    let args = "\"\"\"\nhe said \"hello\n\"\"\", ok)";
    let split = split_top_level_args(args);
    assert_eq!(split.len(), 2);
    assert_eq!(split[1].trim(), "ok");
}

#[test]
fn a_comma_line_inside_a_triple_quoted_string_does_not_split() {
    let s = "\"\"\"\na, b, c\n\"\"\"";
    assert_eq!(count_top_level_commas(s), 0);
    assert_eq!(split_top_level_commas(s), vec![s]);
}

#[test]
fn a_triple_quoted_element_does_not_hide_the_separators_around_it() {
    let s = "a, \"\"\"\nx, y\n\"\"\", b";
    assert_eq!(
        split_top_level_commas(s),
        vec!["a", "\"\"\"\nx, y\n\"\"\"", "b"]
    );
}
