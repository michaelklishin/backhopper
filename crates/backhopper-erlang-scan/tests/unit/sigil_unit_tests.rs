// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! EEP-66 sigil strings: the span primitive and the argument scanners
//! that consult it.

use backhopper_erlang_scan::{
    ScanArity, scan_arity, scan_list_elements, sigil_span, split_top_level_args,
};

fn span_of(src: &str) -> Option<&str> {
    sigil_span(src.as_bytes(), 0).map(|n| &src[..n])
}

#[test]
fn each_paired_delimiter_closes_with_its_partner() {
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')] {
        let src = format!("~s{open}quotes \" commas , parens ( end{close} rest");
        let span = sigil_span(src.as_bytes(), 0).unwrap();
        assert_eq!(
            &src[..span],
            format!("~s{open}quotes \" commas , parens ( end{close}")
        );
    }
}

#[test]
fn each_symmetric_delimiter_closes_on_itself() {
    for d in ['/', '|', '#', '`', '\'', '"'] {
        let src = format!("~X{d}content, with ( and end{d} rest");
        let span = sigil_span(src.as_bytes(), 0).unwrap();
        assert_eq!(&src[..span], format!("~X{d}content, with ( and end{d}"));
    }
}

#[test]
fn a_verbatim_backslash_does_not_escape_the_closer() {
    assert_eq!(span_of(r#"~S"a\" rest"#), Some(r#"~S"a\""#));
}

#[test]
fn escaped_prefixes_do_escape_the_closer() {
    assert_eq!(span_of(r#"~s"a\"b" rest"#), Some(r#"~s"a\"b""#));
    assert_eq!(span_of(r#"~"a\"b" rest"#), Some(r#"~"a\"b""#));
    assert_eq!(span_of(r#"~b"a\"b" rest"#), Some(r#"~b"a\"b""#));
}

#[test]
fn prefix_and_suffix_runs_are_included_in_the_span() {
    assert_eq!(
        span_of("~MYSIGIL/x/suffix1 rest"),
        Some("~MYSIGIL/x/suffix1")
    );
}

#[test]
fn a_tilde_without_a_delimiter_is_not_a_sigil() {
    assert_eq!(span_of("~= x"), None);
    assert_eq!(span_of("~ \"x\""), None);
    assert_eq!(span_of("~"), None);
    assert_eq!(span_of("~name rest"), None);
}

#[test]
fn a_triple_quoted_sigil_ends_at_its_closing_line() {
    let src = "~S\"\"\"\nprose with \" and , and end\n\"\"\"suffix rest";
    assert_eq!(
        span_of(src),
        Some("~S\"\"\"\nprose with \" and , and end\n\"\"\"suffix")
    );
}

#[test]
fn an_unterminated_sigil_spans_to_the_end_of_the_input() {
    assert_eq!(span_of("~s(no closer"), Some("~s(no closer"));
}

#[test]
fn a_latin1_letter_after_the_closer_ends_the_span_before_it() {
    // the crate's name set is ASCII: a Latin-1 suffix byte re-lexes as
    // an atom rather than joining the span
    let src = "~s/x/é";
    assert_eq!(span_of(src), Some("~s/x/"));
}

#[test]
fn sigil_arguments_keep_the_arity() {
    assert_eq!(scan_arity("~s(a \" b), ~S{x , y}, Z)"), ScanArity::Exact(3));
    let args = split_top_level_args("~s(a \" b), ~S{x , y}, Z)");
    assert_eq!(args, vec!["~s(a \" b)", " ~S{x , y}", " Z"]);
}

#[test]
fn a_sigil_element_keeps_the_list_count() {
    match scan_list_elements("~s(a , b), c]") {
        backhopper_erlang_scan::ScannedList::Terminated { elements, .. } => {
            assert_eq!(elements.len(), 2);
        }
        other => panic!("expected terminated list, got {other:?}"),
    }
}

#[test]
fn a_tilde_inside_an_open_string_never_starts_a_sigil() {
    // the string-state arm runs first: the fragment stays string content
    assert_eq!(scan_arity("\"io ~s(\", x)"), ScanArity::Exact(2));
}
