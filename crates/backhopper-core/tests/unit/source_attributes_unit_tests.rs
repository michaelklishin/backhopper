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
    let uses = extract_record_uses("apply(S) -> S#state.machine;\ninit() -> #{leader => Pid}.\n");
    assert_eq!(
        uses.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
        ["state"]
    );
}

#[test]
fn defined_records_are_extracted() {
    let defs = extract_defined_records(
        "-record(state, {first_index, commit_index}).\n-record(cfg, {}).\n",
    );
    assert!(defs.contains("state"));
    assert!(defs.contains("cfg"));
}

#[test]
fn function_signatures_classify_definitions_and_calls() {
    let sigs = extract_function_signatures("apply(A) -> fold(A).\nfold(X) when X > 0 -> X.\n");
    let f = sigs.iter().find(|s| s.name == "apply").unwrap();
    assert_eq!((f.arity, f.is_definition), (1, true));
    let call = sigs
        .iter()
        .find(|s| s.name == "fold" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
    let guarded = sigs
        .iter()
        .find(|s| s.name == "fold" && s.is_definition)
        .unwrap();
    assert_eq!(guarded.arity, 1);
}

#[test]
fn a_binary_literal_argument_counts_as_one_argument() {
    // commas inside `<<...>>` separate binary segments, not call arguments:
    // `id(<<1, 2, 3>>)` is arity 1, and reading it as arity 2 would fire a
    // spurious local-call-undefined-on-target against `id/1`.
    let sigs = extract_function_signatures("dump(Entry) -> id(<<1, 2, 3>>).\nid(V) -> V.\n");
    let call = sigs
        .iter()
        .find(|s| s.name == "id" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
}

#[test]
fn a_variable_application_is_not_a_call() {
    // `Fun(A, B)` calls a variable, not a local function: its lowercase
    // tail must not be re-lexed as `un/2`.
    let sigs = extract_function_signatures("apply(Fun) -> Fun(leader, follower).\n");
    assert!(sigs.iter().all(|s| s.name != "un"));
    assert!(sigs.iter().any(|s| s.name == "apply" && s.is_definition));
}

#[test]
fn a_variable_application_still_exposes_inner_calls() {
    // The variable is skipped whole, but a genuine call among its
    // arguments is still seen.
    let sigs = extract_function_signatures("apply(Fun, Q) -> Fun(queue_definition(Q)).\n");
    let call = sigs
        .iter()
        .find(|s| s.name == "queue_definition" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
    assert!(sigs.iter().all(|s| s.name != "un"));
}

#[test]
fn an_attribute_form_is_not_a_call() {
    let sigs = extract_function_signatures("-export([members/1, process_command/2]).\n");
    assert!(sigs.is_empty());
}

#[test]
fn a_spec_type_is_not_a_call() {
    // `map()` and `term()` in a spec look like zero-arity calls but are
    // type references; the following clause is still a definition.
    let sigs = extract_function_signatures("-spec lookup(map()) -> term().\nlookup(M) -> M.\n");
    assert!(sigs.iter().all(|s| s.name != "map" && s.name != "term"));
    assert!(sigs.iter().any(|s| s.name == "lookup" && s.is_definition));
}

#[test]
fn subtraction_does_not_start_an_attribute() {
    // A mid-expression `-` is an operator, not an attribute marker, so
    // it must not hide the calls around it.
    let sigs = extract_function_signatures("total(X) -> high(X) - low(X).\n");
    assert!(sigs.iter().any(|s| s.name == "high" && !s.is_definition));
    assert!(sigs.iter().any(|s| s.name == "low" && !s.is_definition));
}

#[test]
fn a_define_body_call_is_not_a_call() {
    // Neither the `define` attribute name nor the call in its body is a
    // local call at this site.
    let sigs = extract_function_signatures("-define(MAX, compute(1)).\n");
    assert!(
        sigs.iter()
            .all(|s| s.name != "define" && s.name != "compute")
    );
}

#[test]
fn a_call_after_an_attribute_is_still_found() {
    // The closing `.` ends the attribute, so the following clause and
    // its call are scanned normally.
    let sigs = extract_function_signatures("-spec start() -> ok.\nstart() -> real_call().\n");
    assert!(sigs.iter().any(|s| s.name == "start" && s.is_definition));
    assert!(
        sigs.iter()
            .any(|s| s.name == "real_call" && !s.is_definition)
    );
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
    let src = "init() ->\n    %% ?COMMENTED\n    X = \"?QUOTED\",\n    ?REAL.\n";
    let uses = extract_macro_uses(src);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "REAL");
    assert_eq!(uses[0].line, 4);
}

#[test]
fn macro_use_stringify_form_names_the_macro() {
    let uses = extract_macro_uses("apply() -> ??NAME.\n");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "NAME");
}

#[test]
fn defined_macros_cover_constant_and_function_forms() {
    let src = "-define(DEFAULT_TIMEOUT, 1).\n-define(incr(X), X + 1).\n";
    let defs = extract_defined_macros(src);
    assert!(defs.contains("DEFAULT_TIMEOUT"));
    assert!(defs.contains("incr"));
}

#[test]
fn predefined_macros_are_recognised() {
    assert!(is_predefined_macro("MODULE"));
    assert!(is_predefined_macro("LINE"));
    assert!(!is_predefined_macro("OAUTH2_BOOTSTRAP_PATH"));
}

#[test]
fn extract_behaviours_finds_single_attribute() {
    let src = "-module(ra_server).\n-behaviour(gen_server).\n";
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
    let src = "-module(ra_server).\n\n\n-behaviour(gen_server).\n-behaviour(supervisor).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].line, 4);
    assert_eq!(v[1].line, 5);
}

#[test]
fn extract_includes_recognises_both_forms() {
    let src = "-include(\"include/ra.hrl\").\n-include_lib(\"kernel/include/logger.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 2);
    assert!(matches!(
        v[0].directive,
        IncludeDirective::Include { ref path } if path == "include/ra.hrl"
    ));
    assert!(matches!(
        v[1].directive,
        IncludeDirective::IncludeLib { ref path } if path == "kernel/include/logger.hrl"
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
    let src = "-include  (\n  \"osiris/osiris_log.hrl\"\n).\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].directive.path(), "osiris/osiris_log.hrl");
}

#[test]
fn extract_includes_skips_when_argument_is_not_a_string() {
    let src = "-include({rel, file}).\n";
    let v = extract_includes(src);
    assert!(v.is_empty());
}

#[test]
fn extract_includes_records_line_through_blank_lines() {
    let src = "\n\n-include(\"ra.hrl\").\n";
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
    let src = "-include(\"ra.hrl\").\n-include(\"ra_server.hrl\").\n-include_lib(\"khepri/include/khepri.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].directive.path(), "ra.hrl");
    assert_eq!(v[1].directive.path(), "ra_server.hrl");
    assert_eq!(v[2].directive.path(), "khepri/include/khepri.hrl");
}

#[test]
fn extract_behaviours_finds_three_in_a_row() {
    let src = "-behaviour(gen_server).\n-behaviour(supervisor).\n-behavior(gen_statem).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].behaviour.as_str(), "gen_server");
    assert_eq!(v[1].behaviour.as_str(), "supervisor");
    assert_eq!(v[2].behaviour.as_str(), "gen_statem");
}
