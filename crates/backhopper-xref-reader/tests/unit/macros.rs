// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_xref_graph::{CallTarget, FunctionRef, LocalFunctionRef};
use backhopper_xref_reader::{MacroKey, ModuleData, SourceReader, parse_define};

fn read(source: &str) -> ModuleData {
    let reader = SourceReader::new();
    let (m, _) = reader
        .read_one(&PathBuf::from("test.erl"), source)
        .expect("read_one ok");
    m.expect("expected ModuleData")
}

fn external_names(m: &ModuleData) -> Vec<String> {
    m.external_calls
        .iter()
        .filter_map(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                Some(format!("{}:{}/{}", mfa.module, mfa.function, mfa.arity))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn parse_define_value_macro() {
    let (key, body) = parse_define("(SERVER, my_module)").expect("parse");
    assert_eq!(
        key,
        MacroKey {
            name: "SERVER".into(),
            arity: None,
        }
    );
    assert_eq!(body, "my_module");
}

#[test]
fn parse_define_parameterized_macro() {
    let (key, body) = parse_define("(LOG(Level, Msg), logger:log(Level, Msg))").expect("parse");
    assert_eq!(
        key,
        MacroKey {
            name: "LOG".into(),
            arity: Some(2),
        }
    );
    assert_eq!(body, "logger:log(Level, Msg)");
}

#[test]
fn parse_define_zero_arity_parameterized() {
    let (key, body) = parse_define("(NOW(), erlang:monotonic_time())").expect("parse");
    assert_eq!(
        key,
        MacroKey {
            name: "NOW".into(),
            arity: Some(0),
        }
    );
    assert_eq!(body, "erlang:monotonic_time()");
}

#[test]
fn parse_define_rejects_empty_input() {
    assert!(parse_define("").is_none());
    assert!(parse_define("()").is_none());
}

#[test]
fn value_macro_used_as_module_name_resolves_external_call() {
    let m = read(
        "-module(m).\n\
         -define(SERVER, my_module).\n\
         -export([go/0]).\n\
         go() -> ?SERVER:start(1).\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "my_module:start/1"), "{names:?}");
}

#[test]
fn value_macro_holding_module_colon_function_resolves_external_call() {
    let m = read(
        "-module(m).\n\
         -define(LOGGER, logger:log).\n\
         -export([go/0]).\n\
         go() -> ?LOGGER(debug, \"hi\").\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "logger:log/2"), "{names:?}");
}

#[test]
fn parameterized_macro_expansion_records_call_inside_body() {
    let m = read(
        "-module(m).\n\
         -define(LOG(Level, Msg), logger:log(Level, Msg)).\n\
         -export([go/0]).\n\
         go() -> ?LOG(debug, \"hi\").\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "logger:log/2"), "{names:?}");
}

#[test]
fn parameterized_macro_expansion_runs_per_use_site() {
    let m = read(
        "-module(m).\n\
         -define(LOG(L, M), logger:log(L, M)).\n\
         -export([go/0]).\n\
         go() ->\n\
             ?LOG(debug, \"first\"),\n\
             ?LOG(info, \"second\").\n",
    );
    let occurrences = m
        .external_calls
        .iter()
        .filter(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                mfa.module.as_str() == "logger" && mfa.function.as_str() == "log"
            }
            _ => false,
        })
        .count();
    assert_eq!(
        occurrences, 2,
        "expected one logger:log/2 entry per use site, got {occurrences}"
    );
}

#[test]
fn module_macro_self_call_still_resolves() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> ?MODULE:helper(1).\n\
         helper(X) -> X.\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "m:helper/1"));
}

#[test]
fn unknown_macro_is_dropped_silently() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> ?UNKNOWN.\n",
    );
    assert!(m.external_calls.is_empty());
    assert!(m.unresolved.is_empty());
}

#[test]
fn define_appearing_after_use_site_still_resolves() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> ?SERVER:start(1).\n\
         -define(SERVER, my_module).\n",
    );
    let names = external_names(&m);
    assert!(
        names.iter().any(|n| n == "my_module:start/1"),
        "macro defined after use must still resolve: {names:?}"
    );
}

#[test]
fn parameterized_macro_body_with_local_call_records_local() {
    let m = read(
        "-module(m).\n\
         -define(LOG(X), helper(X)).\n\
         -export([go/0]).\n\
         go() -> ?LOG(value).\n\
         helper(X) -> X.\n",
    );
    assert!(
        m.local_calls.iter().any(|c| matches!(
            &c.callee,
            CallTarget::Local(LocalFunctionRef::Concrete { function, .. })
                if function.as_str() == "helper"
        )),
        "local_calls={:?}",
        m.local_calls
    );
}

#[test]
fn local_call_inside_macro_body_records_at_use_site() {
    let m = read(
        "-module(m).\n\
         -define(WRAP(X), helper(X)).\n\
         -export([go/0]).\n\
         go() -> ?WRAP(value).\n\
         helper(X) -> X.\n",
    );
    let recorded = m.local_calls.iter().any(|c| {
        matches!(
            &c.callee,
            CallTarget::Local(LocalFunctionRef::Concrete { function, arity })
                if function.as_str() == "helper" && arity.get() == 1
        )
    });
    assert!(recorded, "local_calls={:?}", m.local_calls);
}

#[test]
fn nested_call_inside_parameterized_macro_arg_is_recorded() {
    let m = read(
        "-module(m).\n\
         -define(LOG(Level, Msg), logger:log(Level, Msg)).\n\
         -export([go/0]).\n\
         go() -> ?LOG(debug, fmt:format(\"x\", [])).\n",
    );
    let names = external_names(&m);
    assert!(
        names.iter().any(|n| n == "logger:log/2"),
        "macro expanded: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "fmt:format/2"),
        "arg call surfaced: {names:?}"
    );
}
