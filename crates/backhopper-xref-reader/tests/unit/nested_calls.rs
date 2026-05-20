// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_xref_graph::{CallKind, CallTarget, FunctionRef, LocalFunctionRef};
use backhopper_xref_reader::{ModuleData, SourceReader};

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
fn one_level_nested_remote_call_inside_remote_call() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f(b:g()).\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "a:f/1"), "{names:?}");
    assert!(names.iter().any(|n| n == "b:g/0"), "{names:?}");
}

#[test]
fn two_level_nested_remote_calls_chain() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f(b:g(c:h(1))).\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "a:f/1"));
    assert!(names.iter().any(|n| n == "b:g/1"));
    assert!(names.iter().any(|n| n == "c:h/1"));
}

#[test]
fn nested_call_inside_a_tuple_argument() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f({tag, b:g()}).\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "a:f/1"));
    assert!(names.iter().any(|n| n == "b:g/0"));
}

#[test]
fn nested_call_inside_a_list_argument() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f([b:g(), c:h(1)]).\n",
    );
    let names = external_names(&m);
    assert!(names.iter().any(|n| n == "a:f/1"));
    assert!(names.iter().any(|n| n == "b:g/0"));
    assert!(names.iter().any(|n| n == "c:h/1"));
}

#[test]
fn nested_local_call_inside_remote_call_is_recorded_as_local() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f(helper(1)).\n\
         helper(X) -> X.\n",
    );
    assert!(
        m.local_calls
            .iter()
            .any(|c| matches!(&c.callee, CallTarget::Local(LocalFunctionRef::Concrete { function, arity }) if function.as_str() == "helper" && arity.get() == 1)),
        "local_calls={:?}",
        m.local_calls
    );
}

#[test]
fn nested_call_does_not_break_outer_arity_count() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> outer:f(a:g(), b:h(), 1, 2).\n",
    );
    let names = external_names(&m);
    assert!(
        names.iter().any(|n| n == "outer:f/4"),
        "outer arity must still be 4: {names:?}"
    );
    assert!(names.iter().any(|n| n == "a:g/0"));
    assert!(names.iter().any(|n| n == "b:h/0"));
}

#[test]
fn nested_call_inside_apply_arg_list_is_extracted() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> apply(worker, init, [extra:cfg(), shared:env()]).\n",
    );
    let names = external_names(&m);
    assert!(
        names
            .iter()
            .any(|n| n == "worker:init/2" || n.starts_with("worker:init/")),
        "outer apply-resolved call missing: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "extra:cfg/0"),
        "nested inside apply arg list: {names:?}"
    );
    assert!(names.iter().any(|n| n == "shared:env/0"));
}

#[test]
fn nested_call_inside_binary_construction() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> << (a:f())/binary, (b:g())/binary >>.\n",
    );
    let names = external_names(&m);
    assert!(
        names.iter().any(|n| n == "a:f/0"),
        "calls in binary segments: {names:?}"
    );
    assert!(names.iter().any(|n| n == "b:g/0"));
}

#[test]
fn nested_calls_all_carry_the_outer_caller() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> a:f(b:g()).\n",
    );
    for c in &m.external_calls {
        assert_eq!(c.caller.name.as_str(), "go");
        assert_eq!(c.caller.arity.get(), 0);
        assert!(matches!(c.kind, CallKind::Direct));
    }
}
