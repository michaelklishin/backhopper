// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_xref_graph::{CallKind, CallTarget, FunctionRef, LocalFunctionRef};
use backhopper_xref_reader::SourceReader;

fn read(source: &str) -> backhopper_xref_reader::ModuleData {
    let reader = SourceReader::new();
    let (m, _) = reader
        .read_one(&PathBuf::from("test.erl"), source)
        .expect("read_one ok");
    m.expect("expected ModuleData")
}

#[test]
fn external_call_is_recorded() {
    let m = read(
        "-module(caller).\n\
         -export([go/0]).\n\
         go() -> callee:work(1, 2, 3).\n",
    );
    assert_eq!(m.external_calls.len(), 1);
    let CallTarget::External(FunctionRef::Concrete(mfa)) = &m.external_calls[0].callee else {
        panic!("expected external concrete");
    };
    assert_eq!(mfa.module.as_str(), "callee");
    assert_eq!(mfa.function.as_str(), "work");
    assert_eq!(mfa.arity.get(), 3);
}

#[test]
fn local_call_is_recorded() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         a() -> helper(1).\n\
         helper(X) -> X.\n",
    );
    assert_eq!(m.local_calls.len(), 1);
    let CallTarget::Local(LocalFunctionRef::Concrete { function, arity }) =
        &m.local_calls[0].callee
    else {
        panic!("expected local concrete");
    };
    assert_eq!(function.as_str(), "helper");
    assert_eq!(arity.get(), 1);
}

#[test]
fn module_macro_self_call_resolves_to_self_module() {
    let m = read(
        "-module(m).\n\
         -export([a/0, b/0]).\n\
         a() -> ?MODULE:b().\n\
         b() -> ok.\n",
    );
    let ext = m
        .external_calls
        .iter()
        .find(|c| matches!(&c.callee, CallTarget::External(FunctionRef::Concrete(mfa)) if mfa.function.as_str() == "b"))
        .expect("expected ?MODULE:b/0 to be recorded");
    let CallTarget::External(FunctionRef::Concrete(mfa)) = &ext.callee else {
        unreachable!();
    };
    assert_eq!(mfa.module.as_str(), "m");
}

#[test]
fn imported_function_resolves_to_external_call() {
    let m = read(
        "-module(m).\n\
         -import(lists, [map/2]).\n\
         -export([go/1]).\n\
         go(L) -> map(fun (X) -> X end, L).\n",
    );
    let ext = m
        .external_calls
        .iter()
        .find(|c| matches!(&c.callee, CallTarget::External(FunctionRef::Concrete(mfa)) if mfa.module.as_str() == "lists" && mfa.function.as_str() == "map"));
    assert!(
        ext.is_some(),
        "expected imported lists:map/2 to be external"
    );
}

#[test]
fn variable_module_produces_unresolved_module_call() {
    let m = read(
        "-module(m).\n\
         -export([go/2]).\n\
         go(Mod, X) -> Mod:foo(X).\n",
    );
    assert!(!m.unresolved.is_empty());
    let any = m
        .unresolved
        .iter()
        .any(|u| matches!(&u.partial, FunctionRef::UnresolvedModule { function, .. } if function.as_str() == "foo"));
    assert!(any);
}

#[test]
fn variable_function_produces_unresolved_function_call() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(F) -> m1:F().\n",
    );
    let any = m
        .unresolved
        .iter()
        .any(|u| matches!(&u.partial, FunctionRef::UnresolvedFunction { module, .. } if module.as_str() == "m1"));
    assert!(any);
}

#[test]
fn local_call_with_zero_arity_records_arity_zero() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         a() -> helper().\n\
         helper() -> ok.\n",
    );
    let CallTarget::Local(LocalFunctionRef::Concrete { arity, .. }) = &m.local_calls[0].callee
    else {
        panic!();
    };
    assert_eq!(arity.get(), 0);
}

#[test]
fn definitions_are_collected() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         a() -> ok.\n\
         b(X) -> X.\n",
    );
    let names: Vec<String> = m
        .definitions
        .keys()
        .map(|s| format!("{}/{}", s.name.as_str(), s.arity))
        .collect();
    assert_eq!(names, vec!["a/0", "b/1"]);
}

#[test]
fn unexported_definition_becomes_local() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         a() -> ok.\n\
         helper() -> ok.\n",
    );
    assert!(m.exports.iter().any(|s| s.name.as_str() == "a"));
    assert!(m.locals.iter().any(|s| s.name.as_str() == "helper"));
}

#[test]
fn keyword_followed_by_paren_is_not_a_call() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(X) -> case X of 1 -> a(); _ -> b() end.\n\
         a() -> ok.\n\
         b() -> ok.\n",
    );
    let names: Vec<String> = m
        .local_calls
        .iter()
        .filter_map(|c| match &c.callee {
            CallTarget::Local(LocalFunctionRef::Concrete { function, .. }) => {
                Some(function.as_str().to_owned())
            }
            _ => None,
        })
        .collect();
    assert!(names.contains(&"a".to_owned()));
    assert!(names.contains(&"b".to_owned()));
    assert!(!names.contains(&"case".to_owned()));
}

#[test]
fn comments_are_skipped() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         %% callee:fake(1, 2, 3) — this is a comment, not a call.\n\
         a() -> ok.\n",
    );
    assert!(m.external_calls.is_empty());
}

#[test]
fn string_literal_is_not_a_call() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         a() -> \"callee:fake(1, 2)\".\n",
    );
    assert!(m.external_calls.is_empty());
}

#[test]
fn multi_clause_function_is_recorded_once_per_arity() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(0) -> ok;\n\
         go(N) -> go(N - 1).\n",
    );
    let defs: Vec<String> = m
        .definitions
        .keys()
        .map(|s| format!("{}/{}", s.name.as_str(), s.arity))
        .collect();
    assert_eq!(defs, vec!["go/1"]);
}

#[test]
fn function_with_when_guard_is_recorded() {
    let m = read(
        "-module(m).\n\
         -export([f/1]).\n\
         f(N) when N > 0 -> N.\n",
    );
    assert!(m.definitions.keys().any(|s| s.name.as_str() == "f"));
}

#[test]
fn external_call_with_three_arguments_records_arity_three() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:b(1, 2, 3).\n",
    );
    let CallTarget::External(FunctionRef::Concrete(mfa)) = &m.external_calls[0].callee else {
        panic!();
    };
    assert_eq!(mfa.arity.get(), 3);
}

#[test]
fn local_call_with_nested_function_in_arguments_records_outer_arity() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> helper(make(1, 2), 3).\n\
         helper(X, Y) -> {X, Y}.\n\
         make(A, B) -> {A, B}.\n",
    );
    let outer = m
        .local_calls
        .iter()
        .find(|c| {
            matches!(&c.callee, CallTarget::Local(LocalFunctionRef::Concrete { function, .. }) if function.as_str() == "helper")
        })
        .unwrap();
    let CallTarget::Local(LocalFunctionRef::Concrete { arity, .. }) = &outer.callee else {
        panic!();
    };
    assert_eq!(arity.get(), 2);
}

#[test]
fn quoted_atom_function_name_does_not_panic() {
    // Quoted-atom function names are an Erlang oddity. The reader simply
    // skips them: verify no crash and no spurious call site.
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         'fancy name'() -> ok.\n\
         go() -> ok.\n",
    );
    assert!(m.definitions.keys().any(|s| s.name.as_str() == "go"));
}

#[test]
fn try_catch_colon_pattern_is_not_a_call() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() ->\n\
             try ok\n\
             catch C:R:S -> {C, R, S}\n\
             end.\n",
    );
    // `C:R:S` is a pattern in catch clause, must not be read as a remote
    // function reference.
    assert!(m.external_calls.is_empty());
    assert!(m.unresolved.is_empty());
}

#[test]
fn outer_and_inner_calls_in_a_chain_are_both_recorded() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f(b:g()).\n",
    );
    let names: Vec<String> = m
        .external_calls
        .iter()
        .filter_map(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                Some(format!("{}:{}/{}", mfa.module, mfa.function, mfa.arity))
            }
            _ => None,
        })
        .collect();
    assert!(
        names.iter().any(|n| n == "a:f/1"),
        "outer a:f/1 missing: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "b:g/0"),
        "inner b:g/0 missing: {names:?}"
    );
}

#[test]
fn case_expression_branches_extract_calls() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(X) -> case X of one -> a:f(); two -> b:g() end.\n",
    );
    assert_eq!(m.external_calls.len(), 2);
}

#[test]
fn binary_construction_with_calls_inside_is_recorded() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> <<(a:f())/binary>>.\n",
    );
    let names: Vec<String> = m
        .external_calls
        .iter()
        .filter_map(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                Some(format!("{}:{}", mfa.module, mfa.function))
            }
            _ => None,
        })
        .collect();
    assert!(names.contains(&"a:f".to_owned()));
}

#[test]
fn separated_top_level_calls_each_get_their_own_record() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() ->\n\
           a:f(),\n\
           b:g(),\n\
           c:h().\n",
    );
    assert_eq!(m.external_calls.len(), 3);
}

#[test]
fn external_call_carries_kind_direct() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         a() -> callee:work().\n",
    );
    assert_eq!(m.external_calls[0].kind, CallKind::Direct);
}