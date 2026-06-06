// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Value-level checks for `TestSuiteFile<Raw>` and `TestSuiteFile<Parsed>`.
//! Resolve-against-target is exercised in
//! `integration/test_suite_resolve_integration_tests.rs` because it
//! needs a real `TargetTreeIndex` (and therefore a real git repo).
//! `into_reasons` and `into_diagnostic_entry` ride along in the
//! integration test for the same reason.

use backhopper_core::compat::test_suite::{
    HelperCall, MissingHelper, ParseError, TestSuiteFile, is_stdlib_or_otp,
};
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use backhopper_core::model::verdict::TestCallSite;

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn mn(s: &str) -> ModuleName {
    ModuleName::new(s).unwrap()
}

fn fnm(s: &str) -> FunctionName {
    FunctionName::new(s).unwrap()
}

#[test]
fn parse_extracts_simple_helper_call() {
    let src = "-module(some_SUITE).\n\
               x() ->\n    amqp_utils:connection_config(1).\n";
    let parsed = TestSuiteFile::new(rp("deps/rabbit/test/some_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let calls = parsed.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].module.as_str(), "amqp_utils");
    assert_eq!(calls[0].function.as_str(), "connection_config");
    assert_eq!(calls[0].arity, Arity::new(1));
    assert_eq!(calls[0].line, 3);
}

#[test]
fn parse_counts_top_level_args_only() {
    let src = "f() -> mod:fun(a, [b, c], {d, e}).\n";
    let parsed = TestSuiteFile::new(rp("a/b/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert_eq!(parsed.calls().len(), 1);
    assert_eq!(parsed.calls()[0].arity, Arity::new(3));
}

#[test]
fn parse_zero_arity_call() {
    let src = "f() -> mod:fun().\n";
    let parsed = TestSuiteFile::new(rp("a/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert_eq!(parsed.calls()[0].arity, Arity::new(0));
}

#[test]
fn parse_ignores_calls_in_string_or_comment() {
    let src = "% mod:fake(1) in a comment\n\
               f() -> ok = \"mod:other(1)\".\n";
    let parsed = TestSuiteFile::new(rp("a/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert!(parsed.calls().is_empty());
}

#[test]
fn parse_ignores_variable_dispatch() {
    let src = "f(Mod) -> Mod:f(1).\n";
    let parsed = TestSuiteFile::new(rp("a/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert!(parsed.calls().is_empty());
}

#[test]
fn parse_ignores_macro_module_dispatch() {
    let src = "f() -> ?MODULE:f(1).\n";
    let parsed = TestSuiteFile::new(rp("a/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert!(parsed.calls().is_empty());
}

#[test]
fn parse_empty_source_errors() {
    let err = TestSuiteFile::new(rp("a/x_SUITE.erl"), String::new())
        .parse()
        .unwrap_err();
    assert!(matches!(err, ParseError::EmptySource(_)));
}

#[test]
fn parse_whitespace_only_source_errors() {
    let err = TestSuiteFile::new(rp("a/x_SUITE.erl"), "   \n\t\n".into())
        .parse()
        .unwrap_err();
    assert!(matches!(err, ParseError::EmptySource(_)));
}

#[test]
fn parse_records_line_number_through_newlines() {
    let src = "f() ->\n    ok,\n    amqp_utils:open(1).\n";
    let parsed = TestSuiteFile::new(rp("x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert_eq!(parsed.calls()[0].line, 3);
}

#[test]
fn parse_supports_quoted_atom_inside_strings_without_capturing() {
    let src = "f() -> \"escaped:\\\"call\\\"(1)\", a:b(1).\n";
    let parsed = TestSuiteFile::new(rp("x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    assert_eq!(parsed.calls().len(), 1);
    assert_eq!(parsed.calls()[0].module.as_str(), "a");
}

#[test]
fn referenced_modules_dedupes_in_first_occurrence_order() {
    let src = "f() -> a:x(1), b:y(2), a:z(3).\n";
    let parsed = TestSuiteFile::new(rp("a/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let mods = parsed.referenced_modules();
    assert_eq!(mods.len(), 2);
    assert_eq!(mods[0].as_str(), "a");
    assert_eq!(mods[1].as_str(), "b");
}

#[test]
fn is_stdlib_or_otp_covers_common_modules() {
    assert!(is_stdlib_or_otp(&mn("lists")));
    assert!(is_stdlib_or_otp(&mn("erlang")));
    assert!(is_stdlib_or_otp(&mn("ct")));
    assert!(is_stdlib_or_otp(&mn("eunit")));
    assert!(is_stdlib_or_otp(&mn("meck")));
    assert!(!is_stdlib_or_otp(&mn("amqp_utils")));
    assert!(!is_stdlib_or_otp(&mn("rabbit_ct_helpers")));
}

#[test]
fn helper_call_and_missing_helper_value_shape() {
    let c = HelperCall {
        module: mn("amqp_utils"),
        function: fnm("connection_config"),
        arity: Arity::new(1),
        line: 42,
    };
    let m = MissingHelper {
        module: mn("amqp_utils"),
        call_sites: vec![TestCallSite {
            function: fnm("connection_config"),
            arity: Arity::new(1),
            line: 42,
        }],
    };
    assert_eq!(c.module.as_str(), "amqp_utils");
    assert_eq!(m.call_sites.len(), 1);
}

#[test]
fn suite_path_accessor_available_in_raw_state() {
    let raw = TestSuiteFile::new(rp("deps/rabbit/test/x_SUITE.erl"), "x".to_owned());
    assert_eq!(raw.suite_path().as_str(), "deps/rabbit/test/x_SUITE.erl");
}
