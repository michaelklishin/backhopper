// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_xref_reader::SourceReader;

fn read(source: &str) -> backhopper_xref_reader::ModuleData {
    let reader = SourceReader::new();
    let (m, _) = reader
        .read_one(&PathBuf::from("test.erl"), source)
        .expect("read_one ok");
    m.expect("expected ModuleData")
}

#[test]
fn module_attribute_sets_name() {
    let m = read("-module(foo).\n");
    assert_eq!(m.module.as_str(), "foo");
}

#[test]
fn export_attribute_collects_signatures() {
    let m = read(
        "-module(foo).\n\
         -export([a/0, b/2]).\n\
         a() -> ok.\n\
         b(_, _) -> ok.\n",
    );
    let names: Vec<String> = m
        .exports
        .iter()
        .map(|s| format!("{}/{}", s.name.as_str(), s.arity))
        .collect();
    assert_eq!(names, vec!["a/0", "b/2"]);
}

#[test]
fn behaviour_attribute_records_behaviour() {
    let m = read(
        "-module(foo).\n\
         -behaviour(gen_server).\n",
    );
    assert!(m.behaviours.iter().any(|b| b.as_str() == "gen_server"));
}

#[test]
fn callback_attribute_records_required_callback() {
    let m = read(
        "-module(foo).\n\
         -callback handle_call(Req, From, State) -> Reply.\n",
    );
    assert!(m.callbacks.iter().any(|s| s.name.as_str() == "handle_call"));
}

#[test]
fn callback_with_fun_type_arg_counts_arity_correctly() {
    let m = read(
        "-module(foo).\n\
         -callback subscribe(fun((Event) -> ok), Filter) -> ok.\n",
    );
    let sig = m
        .callbacks
        .iter()
        .find(|s| s.name.as_str() == "subscribe")
        .expect("subscribe callback should be recorded");
    assert_eq!(
        sig.arity.get(),
        2,
        "fun-type arg must not be split on its `->`; arity should be 2"
    );
}

#[test]
fn callback_with_nested_parens_in_args_counts_arity_correctly() {
    let m = read(
        "-module(foo).\n\
         -callback handle({op, list({k, v})}, State) -> {ok, State}.\n",
    );
    let sig = m
        .callbacks
        .iter()
        .find(|s| s.name.as_str() == "handle")
        .expect("handle callback should be recorded");
    assert_eq!(sig.arity.get(), 2);
}

#[test]
fn optional_callbacks_attribute_records_optional_set() {
    let m = read(
        "-module(foo).\n\
         -optional_callbacks([code_change/3, format_status/2]).\n",
    );
    assert_eq!(m.optional_callbacks.len(), 2);
}

#[test]
fn import_attribute_resolves_unqualified_calls_to_module() {
    let m = read(
        "-module(foo).\n\
         -import(lists, [map/2]).\n\
         -export([go/1]).\n\
         go(L) -> map(fun (X) -> X end, L).\n",
    );
    assert!(
        m.imports
            .contains_key(&backhopper_core::ModuleName::new("lists".to_owned()).unwrap())
    );
}

#[test]
fn deprecated_two_tuple_defaults_to_eventual_tier() {
    let m = read(
        "-module(foo).\n\
         -deprecated([{a, 0}]).\n",
    );
    let (_, d) = m.deprecated.iter().next().unwrap();
    assert_eq!(d.tier, backhopper_xref_graph::DeprecationTier::Eventual);
}

#[test]
fn deprecated_three_tuple_with_phase_atom_picks_tier() {
    let m = read(
        "-module(foo).\n\
         -deprecated([{a, 0, next_version}]).\n",
    );
    let (_, d) = m.deprecated.iter().next().unwrap();
    assert_eq!(d.tier, backhopper_xref_graph::DeprecationTier::Next);
}

#[test]
fn import_with_empty_function_list_records_no_imports() {
    let m = read(
        "-module(foo).\n\
         -import(lists, []).\n",
    );
    let mod_name = backhopper_core::ModuleName::new("lists".to_owned()).unwrap();
    // The module key may exist with an empty set, or not exist at all.
    assert!(m.imports.get(&mod_name).is_none_or(|s| s.is_empty()));
}

#[test]
fn deprecated_eventually_atom_is_recognised() {
    let m = read(
        "-module(foo).\n\
         -deprecated([{a, 0, eventually}]).\n",
    );
    let (_, d) = m.deprecated.iter().next().unwrap();
    assert_eq!(d.tier, backhopper_xref_graph::DeprecationTier::Eventual);
    assert!(d.message.is_none());
}

#[test]
fn deprecated_next_major_release_atom_is_recognised() {
    let m = read(
        "-module(foo).\n\
         -deprecated([{a, 0, next_major_release}]).\n",
    );
    let (_, d) = m.deprecated.iter().next().unwrap();
    assert_eq!(d.tier, backhopper_xref_graph::DeprecationTier::Next);
}

#[test]
fn deprecated_message_string_is_preserved() {
    let m = read(
        "-module(foo).\n\
         -deprecated([{a, 0, \"use b/0 instead\"}]).\n",
    );
    let (_, d) = m.deprecated.iter().next().unwrap();
    assert_eq!(d.message.as_deref(), Some("use b/0 instead"));
}

#[test]
fn on_load_attribute_records_function() {
    let m = read(
        "-module(foo).\n\
         -on_load({init, 0}).\n",
    );
    let s = m.on_load.unwrap();
    assert_eq!(s.name.as_str(), "init");
}

#[test]
fn behaviour_us_spelling_accepted() {
    let m = read(
        "-module(foo).\n\
         -behavior(gen_server).\n",
    );
    assert!(m.behaviours.iter().any(|b| b.as_str() == "gen_server"));
}

#[test]
fn missing_module_attribute_emits_warning_and_returns_none() {
    let reader = SourceReader::new();
    let (m, w) = reader
        .read_one(&PathBuf::from("test.erl"), "foo() -> ok.\n")
        .expect("read ok");
    assert!(m.is_none());
    assert!(w.iter().any(|w| matches!(
        w,
        backhopper_xref_reader::ReadWarning::NoModuleAttribute { .. }
    )));
}

#[test]
fn empty_source_returns_empty_file_warning() {
    let reader = SourceReader::new();
    let (m, w) = reader
        .read_one(&PathBuf::from("test.erl"), "")
        .expect("read ok");
    assert!(m.is_none());
    assert!(
        w.iter()
            .any(|w| matches!(w, backhopper_xref_reader::ReadWarning::EmptyFile { .. }))
    );
}

#[test]
fn empty_export_list_yields_no_exports() {
    let m = read(
        "-module(m).\n\
         -export([]).\n\
         go() -> ok.\n",
    );
    assert!(m.exports.is_empty());
    // `go/0` is a local because no export covers it.
    assert!(m.locals.iter().any(|s| s.name.as_str() == "go"));
}

#[test]
fn export_list_with_whitespace_around_arity_parses() {
    let m = read(
        "-module(m).\n\
         -export([ a / 0 , b /1 ]).\n\
         a() -> ok.\n\
         b(X) -> X.\n",
    );
    assert_eq!(m.exports.len(), 2);
}

#[test]
fn multiple_export_attributes_accumulate() {
    let m = read(
        "-module(m).\n\
         -export([a/0]).\n\
         -export([b/1]).\n\
         a() -> ok.\n\
         b(X) -> X.\n",
    );
    assert_eq!(m.exports.len(), 2);
}

#[test]
fn multiple_behaviour_attributes_accumulate() {
    let m = read(
        "-module(m).\n\
         -behaviour(gen_server).\n\
         -behaviour(supervisor).\n",
    );
    assert_eq!(m.behaviours.len(), 2);
}

#[test]
fn module_attribute_repeated_with_different_name_emits_conflict() {
    let reader = SourceReader::new();
    let (m, w) = reader
        .read_one(&PathBuf::from("test.erl"), "-module(foo).\n-module(bar).\n")
        .expect("read ok");
    assert_eq!(m.unwrap().module.as_str(), "foo");
    assert!(w.iter().any(|w| matches!(
        w,
        backhopper_xref_reader::ReadWarning::ConflictingModuleAttribute { .. }
    )));
}
