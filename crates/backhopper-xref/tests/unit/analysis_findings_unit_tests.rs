// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Each analysis driven over a fixture to a finding, asserting the typed
//! result, its `is_clean()` verdict, and its rendered `Display`. Fixtures
//! mirror real RabbitMQ-ecosystem modules: `ra_counters` calling `seshat`
//! and OTP, the `ra_machine` behaviour with a `khepri_machine`-style
//! implementer, and `ra_server` calling `ra_log`.

use std::path::PathBuf;

use backhopper_core::{ApplicationName, Arity, FunctionName, Mfa, ModuleName};
use backhopper_xref::{AnalysisResult, ProjectLayout, Vertex, VertexSet, Xref, XrefBuilder};

fn build(sources: &[(&str, &str)]) -> Xref<backhopper_xref::Functions> {
    let mut b = XrefBuilder::new();
    add(&mut b, "ra", sources);
    b.build().unwrap()
}

fn add(b: &mut XrefBuilder, app: &str, sources: &[(&str, &str)]) {
    let files: Vec<(PathBuf, Vec<u8>)> = sources
        .iter()
        .map(|(name, body)| (PathBuf::from(*name), body.as_bytes().to_vec()))
        .collect();
    b.add_application(ApplicationName::new(app.to_owned()).unwrap(), files)
        .unwrap();
}

fn mfa(m: &str, f: &str, a: u8) -> Mfa {
    Mfa::new(
        ModuleName::new(m.to_owned()).unwrap(),
        FunctionName::new(f.to_owned()).unwrap(),
        Arity::new(a),
    )
}

fn mname(s: &str) -> ModuleName {
    ModuleName::new(s.to_owned()).unwrap()
}

// The ra_machine behaviour: init/1 and apply/3 are required, state_enter/2
// is optional. Taken from ra's real -callback set.
const RA_MACHINE: &str = "-module(ra_machine).\n\
    -callback init(Conf :: map()) -> term().\n\
    -callback apply(Meta :: map(), Command :: term(), State) -> {State, term()}.\n\
    -callback state_enter(atom(), term()) -> list().\n\
    -optional_callbacks([state_enter/2]).\n";

#[test]
fn undefined_function_calls_render_and_is_clean() {
    let dirty = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([log/1]).\nlog(E) -> ra_log:append(E).\n",
    )]);
    let u = dirty.undefined_function_calls();
    assert!(!u.is_clean());
    let text = format!("{u}");
    assert!(text.contains("calls undefined"), "{text}");
    assert!(text.contains("ra_log"), "{text}");

    let clean = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([log/1]).\nlog(E) -> ra_log:append(E).\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/1]).\nappend(_) -> ok.\n",
        ),
    ]);
    assert!(clean.undefined_function_calls().is_clean());
}

// ra_counters:init/0 really calls application:ensure_all_started/1, an OTP
// function absent from the graph. Injecting it as a builtin clears the
// otherwise-undefined call.
#[test]
fn with_builtins_suppresses_known_otp_call() {
    let src = &[(
        "ra_counters.erl",
        "-module(ra_counters).\n-export([init/0]).\n\
         init() -> application:ensure_all_started(seshat).\n",
    )];
    let without = build(src);
    assert!(!without.undefined_function_calls().is_clean());

    let mut builtins = VertexSet::new();
    builtins.insert(Vertex::Function(mfa(
        "application",
        "ensure_all_started",
        1,
    )));
    let mut b = XrefBuilder::new().with_builtins(builtins);
    add(&mut b, "ra", src);
    let with = b.build().unwrap();
    assert!(with.undefined_function_calls().is_clean());
}

#[test]
fn locals_not_used_render_and_is_clean() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([init/1]).\n\
         init(Config) -> validate(Config).\n\
         validate(C) -> C.\n\
         stale_helper() -> ok.\n",
    )]);
    let l = x.locals_not_used();
    assert!(!l.is_clean());
    assert!(format!("{l}").contains("unused local: ra_server:stale_helper/0"));
}

// A function named in -on_load is reachable from the runtime even when no
// in-module call references it, so it must not be flagged dead.
#[test]
fn on_load_function_is_not_flagged_unused_local() {
    let x = build(&[(
        "ra_env.erl",
        "-module(ra_env).\n-export([data_dir/0]).\n-on_load({setup, 0}).\n\
         setup() -> ok.\n\
         data_dir() -> \"/var/lib/ra\".\n",
    )]);
    let names: Vec<String> = x
        .locals_not_used()
        .entries
        .iter()
        .map(|u| u.mfa.to_string())
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("ra_env:setup/0")),
        "on_load setup/0 should not be flagged: {names:?}"
    );
}

// An export that satisfies a behaviour callback is kept out of the
// dead-export verdict by the satisfies_callback flag, even when nothing in
// the graph calls it.
#[test]
fn behaviour_callback_export_is_marked_satisfying() {
    let x = build(&[
        ("ra_machine.erl", RA_MACHINE),
        (
            "khepri_machine.erl",
            "-module(khepri_machine).\n-behaviour(ra_machine).\n\
             -export([init/1, apply/3]).\n\
             init(Config) -> Config.\n\
             apply(_Meta, _Command, State) -> {State, ok}.\n",
        ),
    ]);
    let e = x.exports_not_used();
    assert!(!e.is_clean());
    assert!(
        e.entries.iter().all(|u| u.satisfies_callback),
        "every unused export here satisfies a ra_machine callback"
    );
    assert!(format!("{e}").contains("(callback)"));
}

#[test]
fn deprecated_function_calls_render_and_is_clean() {
    let x = build(&[
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/2, write/2]).\n\
             -deprecated([{write, 2, \"use append/2 instead\"}]).\n\
             append(_E, S) -> S.\n\
             write(E, S) -> append(E, S).\n",
        ),
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([log/2]).\n\
             log(E, S) -> ra_log:write(E, S).\n",
        ),
    ]);
    let d = x.deprecated_function_calls();
    assert!(!d.is_clean());
    assert!(format!("{d}").contains("calls deprecated ra_log:write/2"));
}

#[test]
fn called_by_render_and_is_clean() {
    let x = build(&[
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/1]).\nappend(_) -> ok.\n",
        ),
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([log/1]).\nlog(E) -> ra_log:append(E).\n",
        ),
    ]);
    let c = x.called_by(&mfa("ra_log", "append", 1), false);
    assert!(!c.is_clean());
    let text = format!("{c}");
    assert!(text.contains("callers of ra_log:append/1"));
    assert!(text.contains("ra_server:log/1"));

    assert!(x.called_by(&mfa("ra_server", "log", 1), false).is_clean());
}

#[test]
fn calls_from_render_and_is_clean() {
    let x = build(&[
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/1]).\nappend(_) -> ok.\n",
        ),
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([log/1]).\nlog(E) -> ra_log:append(E).\n",
        ),
    ]);
    let c = x.calls_from(&mfa("ra_server", "log", 1), false);
    assert!(!c.is_clean());
    let text = format!("{c}");
    assert!(text.contains("callees of ra_server:log/1"));
    assert!(text.contains("ra_log:append/1"));
}

// Dynamic dispatch through a variable module is unresolved by definition,
// the exact shape ra_server uses to invoke the configured state machine.
#[test]
fn unresolved_calls_render_and_is_clean() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([apply_cmd/3]).\n\
         apply_cmd(Machine, Meta, Cmd) -> Machine:apply(Meta, Cmd).\n",
    )]);
    let r = x.unresolved_calls();
    assert!(!r.is_clean());
    assert!(format!("{r}").contains("ra_server:apply_cmd/3"));
}

// ra_counters really depends on seshat (ra_counters:init/0 ->
// seshat:new_group/1), an honest cross-module edge.
#[test]
fn module_dependencies_is_clean() {
    // The deps/<app>/src layout is what assigns each module to its
    // application; bare file names would leave the application unset.
    let mut b = XrefBuilder::new().with_layout(ProjectLayout::rabbitmq_main());
    add(
        &mut b,
        "ra",
        &[(
            "deps/ra/src/ra_counters.erl",
            "-module(ra_counters).\n-export([init/0]).\n\
             init() -> seshat:new_group(ra).\n",
        )],
    );
    add(
        &mut b,
        "seshat",
        &[(
            "deps/seshat/src/seshat.erl",
            "-module(seshat).\n-export([new_group/1]).\nnew_group(_App) -> ok.\n",
        )],
    );
    let x = b.build().unwrap();

    let deps = x.module_call(&mname("ra_counters"));
    assert!(!deps.is_clean());
    assert!(deps.entries.contains(&mname("seshat")));
    assert!(x.module_call(&mname("seshat")).is_clean());

    let callers = x.module_called_by(&mname("seshat"));
    assert!(!callers.is_clean());
    assert!(callers.entries.contains(&mname("ra_counters")));
}
