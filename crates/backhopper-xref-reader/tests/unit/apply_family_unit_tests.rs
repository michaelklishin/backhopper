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
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> apply(ra_lib, default, [1, 2]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "ra_lib:default/2" && matches!(k, CallKind::Apply)),
        "calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|(s, _)| s.starts_with("erlang:apply")),
        "no apply/3 itself: {calls:?}"
    );
}

#[test]
fn apply_3_list_with_char_literal_comma_counts_one_argument() {
    // `[$,]` is a single-element list (the comma character), so the
    // resolved arity is 1; without $-skipping the interior comma reads
    // as a separator and the call resolves to arity 2.
    let m = read(
        "-module(ra_machine).\n\
         -export([apply/3]).\n\
         apply(_Meta, _Cmd, _State) -> apply(khepri, get, [$,]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "khepri:get/1" && matches!(k, CallKind::Apply)),
        "calls={calls:?}"
    );
}

#[test]
fn spawn_3_resolves_when_args_are_literals() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> spawn(osiris_writer, init, [config]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_4_with_node_strips_leading_node_argument() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(Node) -> spawn(Node, osiris_writer, init, [config]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_link_3_with_literals_resolves() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> spawn_link(osiris_writer, init, [a, b, c]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/3" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_monitor_3_with_literals_resolves() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> spawn_monitor(osiris_writer, init, []).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/0" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_opt_4_with_literals_resolves() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> spawn_opt(osiris_writer, init, [a], [link]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn spawn_opt_5_with_node_strips_leading_node() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(Node) -> spawn_opt(Node, osiris_writer, init, [a], [link]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn hibernate_3_via_erlang_prefix_resolves_self_module() {
    let m = read(
        "-module(ra_server).\n\
         -export([loop/1]).\n\
         loop(State) -> erlang:hibernate(ra_server, loop, [State]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "ra_server:loop/1" && matches!(k, CallKind::Spawn)),
        "{calls:?}"
    );
}

#[test]
fn hibernate_3_bare_form_resolves() {
    let m = read(
        "-module(ra_server).\n\
         -export([loop/1]).\n\
         loop(State) -> hibernate(osiris_writer, loop, [State]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:loop/1" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn apply_3_with_variable_module_records_unresolved() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(Mod) -> apply(Mod, init, [config]).\n",
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
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(F) -> apply(osiris_writer, F, [config]).\n",
    );
    assert!(m.unresolved.iter().any(|u| matches!(
        &u.partial,
        FunctionRef::UnresolvedFunction { module, arity }
            if module.as_str() == "osiris_writer" && arity.map(|a| a.get()) == Some(1)
    ) && matches!(u.kind, CallKind::Apply)));
}

#[test]
fn apply_3_with_variable_args_records_unresolved_without_arity() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(Args) -> apply(osiris_writer, init, Args).\n",
    );
    assert!(
        m.unresolved.iter().any(|u| matches!(
            &u.partial,
            FunctionRef::Concrete(mfa) if mfa.module.as_str() == "osiris_writer" && mfa.function.as_str() == "init"
        ) && matches!(u.kind, CallKind::Apply))
    );
}

#[test]
fn apply_2_falls_through_to_unresolved() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(F) -> apply(F, [config]).\n",
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
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> ra_lib:id(spawn(osiris_writer, init, [])).\n",
    );
    let calls = external_calls(&m);
    assert!(calls.iter().any(|(s, _)| s == "ra_lib:id/1"));
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/0" && matches!(k, CallKind::Spawn))
    );
}

#[test]
fn apply_3_with_empty_arg_list_records_arity_zero() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> apply(osiris_writer, init, []).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/0" && matches!(k, CallKind::Apply))
    );
}

#[test]
fn erlang_qualified_spawn_resolves_to_the_target_not_the_bif() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> erlang:spawn(osiris_writer, init, []).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/0" && matches!(k, CallKind::Spawn)),
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
        "-module(ra_server).\n\
         -export([dispatch/0]).\n\
         dispatch() -> erlang:apply(osiris_writer, init, [config]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls
            .iter()
            .any(|(s, k)| s == "osiris_writer:init/1" && matches!(k, CallKind::Apply)),
        "{calls:?}"
    );
}

#[test]
fn apply_family_resolution_recorded_with_outer_caller_signature() {
    let m = read(
        "-module(ra_server).\n\
         -export([dispatch/1]).\n\
         dispatch(_) -> spawn(osiris_writer, init, []).\n",
    );
    let resolved = m
        .external_calls
        .iter()
        .find(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                mfa.module.as_str() == "osiris_writer"
            }
            _ => false,
        })
        .expect("resolved spawn");
    assert_eq!(resolved.caller.name.as_str(), "dispatch");
    assert_eq!(resolved.caller.arity.get(), 1);
}

// osiris writes a chunk as a single multi-segment binary literal; the
// commas inside `<<...>>` are bit-syntax field separators, not list
// element separators, so apply resolves osiris_log:write to arity 1.
#[test]
fn apply_3_binary_argument_counts_as_one_element() {
    let m = read(
        "-module(osiris_writer).\n\
         -export([write/1]).\n\
         write(ChId) -> apply(osiris_log, write, [<<ChId:64/unsigned, 0:32/unsigned>>]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls.iter().any(|(s, _)| s == "osiris_log:write/1"),
        "calls={calls:?}"
    );
}

// Quoted atoms for the module and function in an apply still resolve to the
// concrete m:f/a.
#[test]
fn apply_3_with_quoted_atom_module_and_function_resolves() {
    let m = read(
        "-module(dispatcher).\n\
         -export([store/0]).\n\
         store() -> apply('khepri', 'put', [k, v]).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls.iter().any(|(s, _)| s == "khepri:put/2"),
        "calls={calls:?}"
    );
}

// An apply nested in another call's argument list is still recorded; the
// outer ra_lib:id/1 and the inner spawned target both surface.
#[test]
fn apply_nested_in_call_argument_is_recorded() {
    let m = read(
        "-module(ra_server).\n\
         -export([boot/0]).\n\
         boot() -> ra_lib:id(apply(osiris_log, init, [cfg])).\n",
    );
    let calls = external_calls(&m);
    assert!(
        calls.iter().any(|(s, _)| s == "ra_lib:id/1"),
        "calls={calls:?}"
    );
    assert!(
        calls.iter().any(|(s, _)| s == "osiris_log:init/1"),
        "calls={calls:?}"
    );
}
