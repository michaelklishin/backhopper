// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Byte-level scanner tests for `compat::source_attributes`. Resolver
//! tests that need a real `TargetTreeIndex` live in
//! `integration/source_attributes_resolve_integration_tests.rs`.

use backhopper_core::compat::source_attributes::{
    declares_parse_transform, extract_behaviours, extract_defined_macros, extract_defined_records,
    extract_function_signatures, extract_imports, extract_includes, extract_macro_uses,
    extract_record_uses, is_predefined_macro,
};
use backhopper_core::model::verdict::IncludeDirective;

#[test]
fn record_uses_skip_maps_and_capture_names() {
    let uses = extract_record_uses("f(S) -> S#state.field;\ng() -> #{k => v}.\n");
    assert_eq!(
        uses.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
        ["state"]
    );
}

#[test]
fn defined_records_are_extracted() {
    let defs = extract_defined_records("-record(state, {a, b}).\n-record(cfg, {}).\n");
    assert!(defs.contains("state"));
    assert!(defs.contains("cfg"));
}

#[test]
fn function_signatures_classify_definitions_and_calls() {
    let sigs = extract_function_signatures("f(A) -> g(A).\ng(X) when X > 0 -> X.\n");
    let f = sigs.iter().find(|s| s.name == "f").unwrap();
    assert_eq!((f.arity, f.is_definition), (1, true));
    let call = sigs
        .iter()
        .find(|s| s.name == "g" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
    let guarded = sigs
        .iter()
        .find(|s| s.name == "g" && s.is_definition)
        .unwrap();
    assert_eq!(guarded.arity, 1);
}

#[test]
fn imports_are_extracted_by_name_and_arity() {
    let imps = extract_imports("-import(lists, [map/2, foldl/3]).\n");
    assert!(imps.contains(&("map".to_owned(), 2)));
    assert!(imps.contains(&("foldl".to_owned(), 3)));
}

#[test]
fn parse_transform_is_detected() {
    assert!(declares_parse_transform(
        "-compile({parse_transform, lager_transform}).\n"
    ));
    assert!(!declares_parse_transform("-compile([debug_info]).\n"));
}

#[test]
fn macro_uses_capture_name_and_line_and_skip_comments_and_strings() {
    let src = "f() ->\n    %% ?COMMENTED\n    X = \"?QUOTED\",\n    ?REAL.\n";
    let uses = extract_macro_uses(src);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "REAL");
    assert_eq!(uses[0].line, 4);
}

#[test]
fn macro_use_stringify_form_names_the_macro() {
    let uses = extract_macro_uses("g() -> ??NAME.\n");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "NAME");
}

#[test]
fn defined_macros_cover_constant_and_function_forms() {
    let src = "-define(FOO, 1).\n-define(bar(X), X + 1).\n";
    let defs = extract_defined_macros(src);
    assert!(defs.contains("FOO"));
    assert!(defs.contains("bar"));
}

#[test]
fn predefined_macros_are_recognised() {
    assert!(is_predefined_macro("MODULE"));
    assert!(is_predefined_macro("LINE"));
    assert!(!is_predefined_macro("OAUTH2_BOOTSTRAP_PATH"));
}

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
