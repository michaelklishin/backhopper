// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A triple-quoted string inside an attribute suspends the
//! ends-with-`.` close rule. Documentation prose ends lines with full
//! stops constantly, so without that the region closes at its first
//! sentence and the rest of the prose is read as code.

use backhopper_core::compat::call_sites::{AttrCtxScanner, extract_qualified_calls, line_context};
use backhopper_core::model::symbol::RefContext;

fn contexts(src: &str) -> Vec<RefContext> {
    line_context(src)
}

fn calls(src: &str) -> Vec<String> {
    let line_map: Vec<u32> = (1..=src.lines().count() as u32).collect();
    extract_qualified_calls(src, &line_map)
        .into_iter()
        .map(|c| c.mfa.to_string())
        .collect()
}

#[test]
fn prose_ending_in_a_full_stop_does_not_close_the_doc_region() {
    let src = "-moduledoc \"\"\"\n\
               Functions for cryptography.\n\
               Seeded once per scheduler.\n\
               \"\"\".\n\
               start() -> ok.\n";
    assert_eq!(
        contexts(src),
        vec![
            RefContext::OtherAttribute,
            RefContext::AttributeString,
            RefContext::AttributeString,
            RefContext::AttributeString,
            RefContext::Body,
        ]
    );
}

#[test]
fn a_call_in_a_documentation_example_is_not_a_call_the_patch_adds() {
    let src = "-doc \"\"\"\n\
               1> crypto:strong_rand_bytes(16).\n\
               \"\"\".\n\
               generate() -> crypto:strong_rand_bytes(32).\n";
    assert_eq!(calls(src), vec!["crypto:strong_rand_bytes/1"]);
}

// the rule is name-independent: a triple-quoted string opens inside any
// attribute form, not only the documentation ones
#[test]
fn a_triple_quoted_define_body_suspends_the_close_rule_too() {
    let src = "-define(BANNER, \"\"\"\n\
               Welcome. Type help. for help.\n\
               \"\"\").\n\
               start() -> ok.\n";
    assert_eq!(
        contexts(src),
        vec![
            RefContext::OtherAttribute,
            RefContext::AttributeString,
            RefContext::AttributeString,
            RefContext::Body,
        ]
    );
}

#[test]
fn a_closer_whose_terminating_dot_sits_on_the_next_line_closes_there() {
    let src = "-doc \"\"\"\n\
               Text.\n\
               \"\"\"\n\
               .\n\
               start() -> ok.\n";
    assert_eq!(
        contexts(src),
        vec![
            RefContext::OtherAttribute,
            RefContext::AttributeString,
            RefContext::AttributeString,
            RefContext::OtherAttribute,
            RefContext::Body,
        ]
    );
}

#[test]
fn an_ordinary_single_line_doc_attribute_still_closes_on_its_own_line() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(
        s.classify("-doc \"Appends an entry.\"."),
        RefContext::OtherAttribute
    );
    assert_eq!(s.classify("append(X) -> X."), RefContext::Body);
}

#[test]
fn a_short_quote_run_at_the_end_of_a_line_does_not_open_a_string() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(
        s.classify("-define(NAME, \"ra\")."),
        RefContext::OtherAttribute
    );
    assert_eq!(s.classify("start() -> ok."), RefContext::Body);
}

#[test]
fn a_longer_closing_run_closes_a_three_quote_opener() {
    let src = "-doc \"\"\"\n\
               Text.\n\
               \"\"\"\"\".\n\
               start() -> ok.\n";
    assert_eq!(contexts(src).last(), Some(&RefContext::Body));
}

#[test]
fn a_closer_shorter_than_the_opener_keeps_the_string_open() {
    let src = "-doc \"\"\"\"\n\
               Text.\n\
               \"\"\".\n\
               still prose.\n\
               \"\"\"\".\n\
               start() -> ok.\n";
    assert_eq!(contexts(src).last(), Some(&RefContext::Body));
    assert_eq!(contexts(src)[2], RefContext::AttributeString);
}
