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
        "a.erl",
        "-module(a).\n-export([go/0]).\ngo() -> missing:f(1).\n",
    )]);
    let u = x.undefined_function_calls();
    assert_eq!(u.entries.len(), 1);
    assert_eq!(u.entries[0].caller, mfa("a", "go", 0));
    // The entry points at the caller's definition, not an empty list.
    assert!(
        !u.entries[0].locations.is_empty(),
        "undefined call should carry the caller's location"
    );
}

#[test]
fn undefined_function_calls_clean_when_target_is_present() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f(1).\n"),
        ("b.erl", "-module(b).\n-export([f/1]).\nf(_) -> ok.\n"),
    ]);
    let u = x.undefined_function_calls();
    assert!(u.entries.is_empty());
}

#[test]
fn exports_not_used_lists_callable_but_uncalled_exports() {
    let x = build(&[
        (
            "a.erl",
            "-module(a).\n-export([go/0, unused/0]).\ngo() -> ok.\nunused() -> ok.\n",
        ),
        (
            "b.erl",
            "-module(b).\n-export([call/0]).\ncall() -> a:go().\n",
        ),
    ]);
    let e = x.exports_not_used();
    let names: Vec<String> = e
        .entries
        .iter()
        .map(|u| format!("{}:{}", u.mfa.module, u.mfa.function))
        .collect();
    assert!(names.contains(&"a:unused".to_owned()));
    assert!(!names.contains(&"a:go".to_owned()));
}

#[test]
fn locals_not_used_lists_uncalled_locals() {
    let x = build(&[(
        "a.erl",
        "-module(a).\n-export([go/0]).\ngo() -> ok.\nhelper() -> ok.\n",
    )]);
    let l = x.locals_not_used();
    assert!(
        l.entries
            .iter()
            .any(|u| u.mfa.function.as_str() == "helper")
    );
}

#[test]
fn called_by_returns_callers_at_one_hop() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let target = mfa("b", "f", 0);
    let c = x.called_by(&target, false);
    assert_eq!(c.entries.len(), 1);
    assert_eq!(c.entries[0].caller, mfa("a", "go", 0));
}

#[test]
fn called_by_transitive_extends_through_chain() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> c:f().\n"),
        ("c.erl", "-module(c).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let target = mfa("c", "f", 0);
    let direct = x.called_by(&target, false);
    let transitive = x.called_by(&target, true);
    assert_eq!(direct.entries.len(), 1);
    assert!(transitive.entries.len() >= 2);
}

#[test]
fn module_called_by_returns_dependent_modules() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let callers = x.module_called_by(&ModuleName::new("b".to_owned()).unwrap());
    assert!(callers.entries.iter().any(|m| m.as_str() == "a"));
}

#[test]
fn deprecated_function_calls_lists_callers() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> old:f().\n"),
        (
            "old.erl",
            "-module(old).\n-export([f/0]).\n-deprecated([{f, 0}]).\nf() -> ok.\n",
        ),
    ]);
    let d = x.deprecated_function_calls();
    assert_eq!(d.entries.len(), 1);
    assert_eq!(d.entries[0].callee.function.as_str(), "f");
}

#[test]
fn recursive_functions_detects_mutual_recursion() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([f/0]).\nf() -> b:g().\n"),
        ("b.erl", "-module(b).\n-export([g/0]).\ng() -> a:f().\n"),
    ]);
    let rec = x.recursive_functions();
    assert!(rec.contains(&mfa("a", "f", 0)));
    assert!(rec.contains(&mfa("b", "g", 0)));
}

#[test]
fn recursive_functions_detects_self_call_via_external_chain() {
    let x = build(&[(
        "a.erl",
        "-module(a).\n-export([loop/0]).\nloop() -> a:loop().\n",
    )]);
    let rec = x.recursive_functions();
    assert!(rec.contains(&mfa("a", "loop", 0)));
}

#[test]
fn module_cycles_finds_two_module_cycle() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> a:go().\n"),
    ]);
    let cycles = x.module_cycles();
    assert_eq!(cycles.len(), 1);
    let names: Vec<&str> = cycles[0].iter().map(ModuleName::as_str).collect();
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn module_cycles_finds_three_module_cycle() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([f/0]).\nf() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> c:f().\n"),
        ("c.erl", "-module(c).\n-export([f/0]).\nf() -> a:f().\n"),
    ]);
    let cycles = x.module_cycles();
    assert_eq!(cycles.len(), 1);
    let names: Vec<&str> = cycles[0].iter().map(ModuleName::as_str).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn module_cycles_empty_for_dag() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    assert!(x.module_cycles().is_empty());
}

#[test]
fn nonconformant_implementers_lists_missing_required_callbacks() {
    let x = build(&[
        (
            "my_beh.erl",
            "-module(my_beh).\n-callback do(X) -> ok.\n-callback also(X, Y) -> ok.\n",
        ),
        (
            "good.erl",
            "-module(good).\n-behaviour(my_beh).\n-export([do/1, also/2]).\ndo(_) -> ok.\nalso(_, _) -> ok.\n",
        ),
        (
            "bad.erl",
            "-module(bad).\n-behaviour(my_beh).\n-export([do/1]).\ndo(_) -> ok.\n",
        ),
    ]);
    let nc = x.nonconformant_implementers();
    assert_eq!(nc.len(), 1);
    assert_eq!(nc[0].implementer.as_str(), "bad");
    assert!(nc[0].missing.iter().any(|s| s.name.as_str() == "also"));
}

#[test]
fn calls_from_returns_callees_at_one_hop() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let source = mfa("a", "go", 0);
    let c = x.calls_from(&source, false);
    assert_eq!(c.entries.len(), 1);
}

#[test]
fn called_by_empty_for_unreachable_target() {
    let x = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let target = mfa("a", "f", 0);
    let c = x.called_by(&target, false);
    assert!(c.entries.is_empty());
}

#[test]
fn undefined_functions_distinct_from_undefined_function_calls() {
    let x = build(&[(
        "a.erl",
        "-module(a).\n-export([go/0]).\ngo() ->\n  m:f(),\n  m:f(),\n  m:g().\n",
    )]);
    let unique = x.undefined_functions();
    let with_callers = x.undefined_function_calls();
    assert_eq!(unique.len(), 2);
    assert_eq!(with_callers.entries.len(), 2);
}

#[test]
fn unresolved_calls_inventory_lists_variable_targets() {
    let x = build(&[(
        "a.erl",
        "-module(a).\n-export([go/2]).\ngo(Mod, X) -> Mod:foo(X).\n",
    )]);
    let r = x.unresolved_calls();
    assert!(!r.entries.is_empty());
}

#[test]
fn unresolved_calls_inventory_empty_when_no_dynamic_dispatch() {
    let x = build(&[("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n")]);
    let r = x.unresolved_calls();
    assert!(r.entries.is_empty());
}

#[test]
fn locals_not_used_empty_when_all_locals_called() {
    let x = build(&[(
        "a.erl",
        "-module(a).\n-export([go/0]).\ngo() -> helper().\nhelper() -> ok.\n",
    )]);
    let l = x.locals_not_used();
    assert!(l.entries.is_empty());
}

#[test]
fn exports_not_used_marks_on_load_function() {
    let x = build(&[(
        "a.erl",
        "-module(a).\n-export([init/0]).\n-on_load({init, 0}).\ninit() -> ok.\n",
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
    let app1 = ApplicationName::new("a1".to_owned()).unwrap();
    let app2 = ApplicationName::new("a2".to_owned()).unwrap();
    b.add_application(
        app1.clone(),
        vec![(
            PathBuf::from("deps/a1/src/m1.erl"),
            b"-module(m1).\n-export([f/0]).\nf() -> m2:g().\n".to_vec(),
        )],
    )
    .unwrap();
    b.add_application(
        app2,
        vec![(
            PathBuf::from("deps/a2/src/m2.erl"),
            b"-module(m2).\n-export([g/0]).\ng() -> ok.\n".to_vec(),
        )],
    )
    .unwrap();
    let x = b.build().unwrap();
    let deps = x.application_call(&app1);
    assert!(deps.entries.iter().any(|a| a.as_str() == "a2"));
}

#[test]
fn implementers_of_returns_all_implementers_sorted() {
    let x = build(&[
        (
            "z_server.erl",
            "-module(z_server).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
        ),
        (
            "a_server.erl",
            "-module(a_server).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
        ),
    ]);
    let impls =
        x.implementers_of(&backhopper_core::BehaviourName::new("gen_server".to_owned()).unwrap());
    let names: Vec<&str> = impls.iter().map(ModuleName::as_str).collect();
    assert_eq!(names, vec!["a_server", "z_server"]);
}

#[test]
fn implementers_of_empty_for_unknown_behaviour() {
    let x = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let impls =
        x.implementers_of(&backhopper_core::BehaviourName::new("nonexistent".to_owned()).unwrap());
    assert!(impls.is_empty());
}

#[test]
fn module_call_forward_returns_dependencies() {
    let x = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let deps = x.module_call(&ModuleName::new("a".to_owned()).unwrap());
    assert!(deps.entries.iter().any(|m| m.as_str() == "b"));
}

#[test]
fn module_call_empty_for_independent_module() {
    let x = build(&[("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n")]);
    let deps = x.module_call(&ModuleName::new("a".to_owned()).unwrap());
    assert!(deps.entries.is_empty());
}

#[test]
fn implementers_of_returns_modules_with_behaviour_attribute() {
    let x = build(&[(
        "my_server.erl",
        "-module(my_server).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
    )]);
    let impls =
        x.implementers_of(&backhopper_core::BehaviourName::new("gen_server".to_owned()).unwrap());
    assert!(impls.iter().any(|m| m.as_str() == "my_server"));
}
