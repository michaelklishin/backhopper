// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_xref_graph::{CallKind, CallTarget, FunctionRef};
use backhopper_xref_reader::{ModuleData, SourceReader};

fn read(source: &str) -> ModuleData {
    let reader = SourceReader::new();
    let (m, _) = reader
        .read_one(&PathBuf::from("test.erl"), source)
        .expect("read_one ok");
    m.expect("expected ModuleData")
}

fn external_calls(m: &ModuleData) -> Vec<(String, CallKind)> {
    m.external_calls
        .iter()
        .filter_map(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => Some((
                format!("{}:{}/{}", mfa.module, mfa.function, mfa.arity),
                c.kind,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn apply_3_with_atom_literals_resolves_to_concrete_mfa() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> apply(foo, bar, [1, 2]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "foo:bar/2" && matches!(k, CallKind::Apply)),
        "calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|(s, _)| s.starts_with("erlang:apply")),
        "no apply/3 itself: {calls:?}"
    );
}

#[test]
fn spawn_3_resolves_when_args_are_literals() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> spawn(worker, init, [config]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_4_with_node_strips_leading_node_argument() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(Node) -> spawn(Node, worker, init, [config]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_link_3_with_literals_resolves() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> spawn_link(worker, init, [a, b, c]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/3" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_monitor_3_with_literals_resolves() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> spawn_monitor(worker, init, []).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/0" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_opt_4_with_literals_resolves() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> spawn_opt(worker, init, [a], [link]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_opt_5_with_node_strips_leading_node() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(Node) -> spawn_opt(Node, worker, init, [a], [link]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn hibernate_3_via_erlang_prefix_resolves_self_module() {
    let m = read(
        "-module(m).\n\
         -export([loop/1]).\n\
         loop(State) -> erlang:hibernate(m, loop, [State]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "m:loop/1" && matches!(k, CallKind::Spawn)),
        "{calls:?}"
    );
}

#[test]
fn hibernate_3_bare_form_resolves() {
    let m = read(
        "-module(m).\n\
         -export([loop/1]).\n\
         loop(State) -> hibernate(worker, loop, [State]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:loop/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn apply_3_with_variable_module_records_unresolved() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(Mod) -> apply(Mod, init, [config]).\n",
    );
    assert!(
        m.unresolved.iter().any(|u| matches!(
            &u.partial,
            FunctionRef::UnresolvedModule { function, arity }
                if function.as_str() == "init" && arity.map(|a| a.get()) == Some(1)
        ) && matches!(u.kind, CallKind::Apply)),
        "unresolved={:?}",
        m.unresolved
    );
}

#[test]
fn apply_3_with_variable_function_records_unresolved() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(F) -> apply(worker, F, [config]).\n",
    );
    assert!(m.unresolved.iter().any(|u| matches!(
        &u.partial,
        FunctionRef::UnresolvedFunction { module, arity }
            if module.as_str() == "worker" && arity.map(|a| a.get()) == Some(1)
    ) && matches!(u.kind, CallKind::Apply)));
}

#[test]
fn apply_3_with_variable_args_records_unresolved_without_arity() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(Args) -> apply(worker, init, Args).\n",
    );
    assert!(
        m.unresolved.iter().any(|u| matches!(
            &u.partial,
            FunctionRef::Concrete(mfa) if mfa.module.as_str() == "worker" && mfa.function.as_str() == "init"
        ) && matches!(u.kind, CallKind::Apply))
    );
}

#[test]
fn apply_2_falls_through_to_unresolved() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(F) -> apply(F, [config]).\n",
    );
    assert!(
        m.unresolved
            .iter()
            .any(|u| matches!(u.kind, CallKind::Apply)),
        "apply/2 should still produce an Apply-kind unresolved"
    );
}

#[test]
fn nested_apply_inside_other_call_resolves() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> outer:run(spawn(worker, init, [])).\n",
    );
    let calls = external_calls(&m);
    assert!(calls.iter().any(|(s, _)| s == "outer:run/1"));
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/0" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn apply_3_with_empty_arg_list_records_arity_zero() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> apply(worker, init, []).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/0" && matches!(k, CallKind::Apply))
    );
}

#[test]
fn erlang_qualified_spawn_resolves_to_the_target_not_the_bif() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> erlang:spawn(worker, init, []).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/0" && matches!(k, CallKind::Spawn)),
        "spawn target resolved: {calls:?}"
    );
    assert!(
        !calls.iter().any(|(s, _)| s == "erlang:spawn/3"),
        "the BIF itself shouldn't appear: {calls:?}"
    );
}

#[test]
fn erlang_qualified_apply_resolves_to_the_target() {
    let m = read(
        "-module(m).\n\
         -export([go/0]).\n\
         go() -> erlang:apply(worker, init, [config]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "worker:init/1" && matches!(k, CallKind::Apply)),
        "{calls:?}"
    );
}

#[test]
fn apply_family_resolution_recorded_with_outer_caller_signature() {
    let m = read(
        "-module(m).\n\
         -export([go/1]).\n\
         go(_) -> spawn(worker, init, []).\n",
    );
    let resolved = m
        .external_calls
        .iter()
        .find(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => mfa.module.as_str() == "worker",
            _ => false,
        })
        .expect("resolved spawn");
    assert_eq!(resolved.caller.name.as_str(), "go");
    assert_eq!(resolved.caller.arity.get(), 1);
}
