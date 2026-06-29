// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::{ApplicationName, Arity, FunctionName, Mfa, ModuleName};
use backhopper_xref::{Xref, XrefBuilder};

fn build(sources: &[(&str, &str)]) -> Xref<backhopper_xref::Functions> {
    let mut b = XrefBuilder::new();
    let app = ApplicationName::new("test".to_owned()).unwrap();
    let files: Vec<(PathBuf, Vec<u8>)> = sources
        .iter()
        .map(|(name, body)| (PathBuf::from(*name), body.as_bytes().to_vec()))
        .collect();
    b.add_application(app, files).unwrap();
    b.build().unwrap()
}

fn mfa(m: &str, f: &str, a: u8) -> Mfa {
    Mfa::new(
        ModuleName::new(m.to_owned()).unwrap(),
        FunctionName::new(f.to_owned()).unwrap(),
        Arity::new(a),
    )
}

#[test]
fn undefined_function_calls_flags_missing_target() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([init/0]).\ninit() -> ra_log:append(1).\n",
    )]);
    let u = x.undefined_function_calls();
    assert_eq!(u.entries.len(), 1);
    assert_eq!(u.entries[0].caller, mfa("ra_server", "init", 0));
    // The entry points at the caller's definition, not an empty list.
    assert!(
        !u.entries[0].locations.is_empty(),
        "undefined call should carry the caller's location"
    );
}

#[test]
fn undefined_function_calls_clean_when_target_is_present() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([init/0]).\ninit() -> ra_log:append(1).\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/1]).\nappend(_) -> ok.\n",
        ),
    ]);
    let u = x.undefined_function_calls();
    assert!(u.entries.is_empty());
}

#[test]
fn exports_not_used_lists_callable_but_uncalled_exports() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([recover/0, snapshot/0]).\nrecover() -> ok.\nsnapshot() -> ok.\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([restore/0]).\nrestore() -> ra_server:recover().\n",
        ),
    ]);
    let e = x.exports_not_used();
    let names: Vec<String> = e
        .entries
        .iter()
        .map(|u| format!("{}:{}", u.mfa.module, u.mfa.function))
        .collect();
    assert!(names.contains(&"ra_server:snapshot".to_owned()));
    assert!(!names.contains(&"ra_server:recover".to_owned()));
}

#[test]
fn locals_not_used_lists_uncalled_locals() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([recover/0]).\nrecover() -> ok.\nflush() -> ok.\n",
    )]);
    let l = x.locals_not_used();
    assert!(l.entries.iter().any(|u| u.mfa.function.as_str() == "flush"));
}

#[test]
fn called_by_returns_callers_at_one_hop() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ok.\n",
        ),
    ]);
    let target = mfa("ra_log", "append", 0);
    let c = x.called_by(&target, false);
    assert_eq!(c.entries.len(), 1);
    assert_eq!(c.entries[0].caller, mfa("ra_server", "handle", 0));
}

#[test]
fn called_by_transitive_extends_through_chain() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ra_machine:apply().\n",
        ),
        (
            "ra_machine.erl",
            "-module(ra_machine).\n-export([apply/0]).\napply() -> ok.\n",
        ),
    ]);
    let target = mfa("ra_machine", "apply", 0);
    let direct = x.called_by(&target, false);
    let transitive = x.called_by(&target, true);
    assert_eq!(direct.entries.len(), 1);
    assert!(transitive.entries.len() >= 2);
}

#[test]
fn module_called_by_returns_dependent_modules() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ok.\n",
        ),
    ]);
    let callers = x.module_called_by(&ModuleName::new("ra_log".to_owned()).unwrap());
    assert!(callers.entries.iter().any(|m| m.as_str() == "ra_server"));
}

#[test]
fn deprecated_function_calls_lists_callers() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_lib:dump().\n",
        ),
        (
            "ra_lib.erl",
            "-module(ra_lib).\n-export([dump/0]).\n-deprecated([{dump, 0}]).\ndump() -> ok.\n",
        ),
    ]);
    let d = x.deprecated_function_calls();
    assert_eq!(d.entries.len(), 1);
    assert_eq!(d.entries[0].callee.function.as_str(), "dump");
}

#[test]
fn recursive_functions_detects_mutual_recursion() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([loop/0]).\nloop() -> ra_log:tick().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([tick/0]).\ntick() -> ra_server:loop().\n",
        ),
    ]);
    let rec = x.recursive_functions();
    assert!(rec.contains(&mfa("ra_server", "loop", 0)));
    assert!(rec.contains(&mfa("ra_log", "tick", 0)));
}

#[test]
fn recursive_functions_detects_self_call_via_external_chain() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([loop/0]).\nloop() -> ra_server:loop().\n",
    )]);
    let rec = x.recursive_functions();
    assert!(rec.contains(&mfa("ra_server", "loop", 0)));
}

#[test]
fn module_cycles_finds_two_module_cycle() {
    let x = build(&[
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ra_server:handle().\n",
        ),
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
    ]);
    let cycles = x.module_cycles();
    assert_eq!(cycles.len(), 1);
    let names: Vec<&str> = cycles[0].iter().map(ModuleName::as_str).collect();
    assert_eq!(names, vec!["ra_log", "ra_server"]);
}

#[test]
fn module_cycles_finds_three_module_cycle() {
    let x = build(&[
        (
            "ra_directory.erl",
            "-module(ra_directory).\n-export([init/0]).\ninit() -> ra_log:init().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([init/0]).\ninit() -> ra_machine:init().\n",
        ),
        (
            "ra_machine.erl",
            "-module(ra_machine).\n-export([init/0]).\ninit() -> ra_directory:init().\n",
        ),
    ]);
    let cycles = x.module_cycles();
    assert_eq!(cycles.len(), 1);
    let names: Vec<&str> = cycles[0].iter().map(ModuleName::as_str).collect();
    assert_eq!(names, vec!["ra_directory", "ra_log", "ra_machine"]);
}

#[test]
fn module_cycles_empty_for_dag() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ok.\n",
        ),
    ]);
    assert!(x.module_cycles().is_empty());
}

#[test]
fn nonconformant_implementers_lists_missing_required_callbacks() {
    let x = build(&[
        (
            "ra_machine.erl",
            "-module(ra_machine).\n-callback init(X) -> ok.\n-callback apply(X, Y) -> ok.\n",
        ),
        (
            "rabbit_fifo.erl",
            "-module(rabbit_fifo).\n-behaviour(ra_machine).\n-export([init/1, apply/2]).\ninit(_) -> ok.\napply(_, _) -> ok.\n",
        ),
        (
            "rabbit_quorum_queue.erl",
            "-module(rabbit_quorum_queue).\n-behaviour(ra_machine).\n-export([init/1]).\ninit(_) -> ok.\n",
        ),
    ]);
    let nc = x.nonconformant_implementers();
    assert_eq!(nc.len(), 1);
    assert_eq!(nc[0].implementer.as_str(), "rabbit_quorum_queue");
    assert!(nc[0].missing.iter().any(|s| s.name.as_str() == "apply"));
}

#[test]
fn calls_from_returns_callees_at_one_hop() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ok.\n",
        ),
    ]);
    let source = mfa("ra_server", "handle", 0);
    let c = x.calls_from(&source, false);
    assert_eq!(c.entries.len(), 1);
}

#[test]
fn called_by_empty_for_unreachable_target() {
    let x = build(&[(
        "ra_log.erl",
        "-module(ra_log).\n-export([append/0]).\nappend() -> ok.\n",
    )]);
    let target = mfa("ra_log", "append", 0);
    let c = x.called_by(&target, false);
    assert!(c.entries.is_empty());
}

#[test]
fn undefined_functions_distinct_from_undefined_function_calls() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([recover/0]).\nrecover() ->\n  ra_log:append(),\n  ra_log:append(),\n  ra_log:fold().\n",
    )]);
    let unique = x.undefined_functions();
    let with_callers = x.undefined_function_calls();
    assert_eq!(unique.len(), 2);
    assert_eq!(with_callers.entries.len(), 2);
}

#[test]
fn unresolved_calls_inventory_lists_variable_targets() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([dispatch/2]).\ndispatch(Mod, X) -> Mod:apply(X).\n",
    )]);
    let r = x.unresolved_calls();
    assert!(!r.entries.is_empty());
}

#[test]
fn unresolved_calls_inventory_empty_when_no_dynamic_dispatch() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
    )]);
    let r = x.unresolved_calls();
    assert!(r.entries.is_empty());
}

#[test]
fn locals_not_used_empty_when_all_locals_called() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([recover/0]).\nrecover() -> flush().\nflush() -> ok.\n",
    )]);
    let l = x.locals_not_used();
    assert!(l.entries.is_empty());
}

#[test]
fn exports_not_used_marks_on_load_function() {
    let x = build(&[(
        "ra_counters.erl",
        "-module(ra_counters).\n-export([init/0]).\n-on_load({init, 0}).\ninit() -> ok.\n",
    )]);
    let e = x.exports_not_used();
    let entry = e
        .entries
        .iter()
        .find(|u| u.mfa.function.as_str() == "init")
        .unwrap();
    assert!(entry.is_on_load);
}

#[test]
fn application_call_returns_typed_app_dependencies() {
    use backhopper_xref::ProjectLayout;
    let mut b = XrefBuilder::new().with_layout(ProjectLayout::rabbitmq_main());
    let ra = ApplicationName::new("ra".to_owned()).unwrap();
    let khepri = ApplicationName::new("khepri".to_owned()).unwrap();
    b.add_application(
        ra.clone(),
        vec![(
            PathBuf::from("deps/ra/src/ra_server.erl"),
            b"-module(ra_server).\n-export([handle/0]).\nhandle() -> khepri_machine:get().\n"
                .to_vec(),
        )],
    )
    .unwrap();
    b.add_application(
        khepri,
        vec![(
            PathBuf::from("deps/khepri/src/khepri_machine.erl"),
            b"-module(khepri_machine).\n-export([get/0]).\nget() -> ok.\n".to_vec(),
        )],
    )
    .unwrap();
    let x = b.build().unwrap();
    let deps = x.application_call(&ra);
    assert!(deps.entries.iter().any(|a| a.as_str() == "khepri"));
}

#[test]
fn implementers_of_returns_all_implementers_sorted() {
    let x = build(&[
        (
            "ra_server_proc.erl",
            "-module(ra_server_proc).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
        ),
        (
            "osiris_writer.erl",
            "-module(osiris_writer).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
        ),
    ]);
    let impls =
        x.implementers_of(&backhopper_core::BehaviourName::new("gen_server".to_owned()).unwrap());
    let names: Vec<&str> = impls.iter().map(ModuleName::as_str).collect();
    assert_eq!(names, vec!["osiris_writer", "ra_server_proc"]);
}

#[test]
fn implementers_of_empty_for_unknown_behaviour() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([handle/0]).\nhandle() -> ok.\n",
    )]);
    let impls =
        x.implementers_of(&backhopper_core::BehaviourName::new("nonexistent".to_owned()).unwrap());
    assert!(impls.is_empty());
}

#[test]
fn module_call_forward_returns_dependencies() {
    let x = build(&[
        (
            "ra_server.erl",
            "-module(ra_server).\n-export([handle/0]).\nhandle() -> ra_log:append().\n",
        ),
        (
            "ra_log.erl",
            "-module(ra_log).\n-export([append/0]).\nappend() -> ok.\n",
        ),
    ]);
    let deps = x.module_call(&ModuleName::new("ra_server".to_owned()).unwrap());
    assert!(deps.entries.iter().any(|m| m.as_str() == "ra_log"));
}

#[test]
fn module_call_empty_for_independent_module() {
    let x = build(&[(
        "ra_server.erl",
        "-module(ra_server).\n-export([handle/0]).\nhandle() -> ok.\n",
    )]);
    let deps = x.module_call(&ModuleName::new("ra_server".to_owned()).unwrap());
    assert!(deps.entries.is_empty());
}

#[test]
fn implementers_of_returns_modules_with_behaviour_attribute() {
    let x = build(&[(
        "ra_server_proc.erl",
        "-module(ra_server_proc).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
    )]);
    let impls =
        x.implementers_of(&backhopper_core::BehaviourName::new("gen_server".to_owned()).unwrap());
    assert!(impls.iter().any(|m| m.as_str() == "ra_server_proc"));
}
