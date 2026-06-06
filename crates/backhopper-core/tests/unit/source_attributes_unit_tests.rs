// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Byte-level scanner tests for `compat::source_attributes`. Resolver
//! tests that need a real `TargetTreeIndex` live in
//! `integration/source_attributes_resolve_integration_tests.rs`.

use backhopper_core::compat::source_attributes::{extract_behaviours, extract_includes};
use backhopper_core::model::verdict::IncludeDirective;

#[test]
fn extract_behaviours_finds_single_attribute() {
    let src = "-module(x).\n-behaviour(gen_server).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].behaviour.as_str(), "gen_server");
    assert_eq!(v[0].line, 2);
}

#[test]
fn extract_behaviours_accepts_us_spelling() {
    let src = "-behavior(gen_statem).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].behaviour.as_str(), "gen_statem");
}

#[test]
fn extract_behaviours_handles_quoted_atom() {
    let src = "-behaviour('Quoted_atom').\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].behaviour.as_str(), "Quoted_atom");
}

#[test]
fn extract_behaviours_skips_in_comment_or_string() {
    let src = "% -behaviour(commented).\n\"-behaviour(\\\"string\\\")\".\n";
    let v = extract_behaviours(src);
    assert!(v.is_empty());
}

#[test]
fn extract_behaviours_records_line_for_each() {
    let src = "-module(x).\n\n\n-behaviour(a).\n-behaviour(b).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].line, 4);
    assert_eq!(v[1].line, 5);
}

#[test]
fn extract_includes_recognises_both_forms() {
    let src = "-include(\"rel/x.hrl\").\n-include_lib(\"app/include/y.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 2);
    assert!(matches!(
        v[0].directive,
        IncludeDirective::Include { ref path } if path == "rel/x.hrl"
    ));
    assert!(matches!(
        v[1].directive,
        IncludeDirective::IncludeLib { ref path } if path == "app/include/y.hrl"
    ));
}

#[test]
fn extract_includes_skips_macro_value_form() {
    let src = "-include(?HEADER).\n";
    let v = extract_includes(src);
    assert!(v.is_empty());
}

#[test]
fn extract_includes_handles_attribute_with_extra_whitespace() {
    let src = "-include  (\n  \"a/b.hrl\"\n).\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].directive.path(), "a/b.hrl");
}

#[test]
fn extract_includes_skips_when_argument_is_not_a_string() {
    let src = "-include({rel, file}).\n";
    let v = extract_includes(src);
    assert!(v.is_empty());
}

#[test]
fn extract_includes_records_line_through_blank_lines() {
    let src = "\n\n-include(\"a.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].line, 3);
}

#[test]
fn empty_source_yields_no_results() {
    assert!(extract_behaviours("").is_empty());
    assert!(extract_includes("").is_empty());
}

#[test]
fn extractors_do_not_panic_on_unmatched_parens() {
    let src = "-include(\"unfinished\n";
    let _ = extract_includes(src);
    let _ = extract_behaviours(src);
}

#[test]
fn extract_includes_finds_consecutive_attributes_on_separate_lines() {
    let src = "-include(\"a.hrl\").\n-include(\"b.hrl\").\n-include_lib(\"app/c.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].directive.path(), "a.hrl");
    assert_eq!(v[1].directive.path(), "b.hrl");
    assert_eq!(v[2].directive.path(), "app/c.hrl");
}

#[test]
fn extract_behaviours_finds_three_in_a_row() {
    let src = "-behaviour(a).\n-behaviour(b).\n-behavior(c).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].behaviour.as_str(), "a");
    assert_eq!(v[1].behaviour.as_str(), "b");
    assert_eq!(v[2].behaviour.as_str(), "c");
}
