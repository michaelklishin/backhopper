// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-export_type` and type-declaration extraction. The key
//! case is `export_all`: it exports functions, not types, so it must
//! not make the exported-type set incomplete.

use backhopper_core::compat::source_attributes::{extract_defined_types, extract_exported_types};
use backhopper_core::model::names::{Arity, TypeName};

fn key(name: &str, arity: u8) -> (TypeName, Arity) {
    (TypeName::new(name).unwrap(), Arity::new(arity))
}

#[test]
fn an_export_type_list_is_read_with_names_and_arities() {
    let src = "-module(rabbit_net).\n-export_type([socket/0, proxy_socket/0, hostname/0]).\n";
    let set = extract_exported_types(src);
    assert!(set.complete);
    let names: Vec<_> = set
        .types
        .iter()
        .map(|t| (t.name.as_str(), t.arity.get()))
        .collect();
    assert_eq!(
        names,
        vec![("socket", 0), ("proxy_socket", 0), ("hostname", 0)]
    );
}

#[test]
fn an_export_type_entry_carries_its_line() {
    let src = "-module(rabbit_net).\n\n-export_type([socket/0]).\n";
    let set = extract_exported_types(src);
    assert_eq!(set.types[0].line, 3);
}

#[test]
fn export_all_does_not_make_the_exported_type_set_incomplete() {
    // export_all exports functions, not types: it cannot hide a type.
    let src = "-module(rabbit_net).\n-compile(export_all).\n-export_type([socket/0]).\n";
    let set = extract_exported_types(src);
    assert!(set.complete);
    assert_eq!(set.types.len(), 1);
}

#[test]
fn nowarn_export_all_does_not_make_the_exported_type_set_incomplete() {
    let src = "-module(rabbit_net).\n-compile(nowarn_export_all).\n-export_type([socket/0]).\n";
    assert!(extract_exported_types(src).complete);
}

#[test]
fn a_parse_transform_makes_the_exported_type_set_incomplete() {
    let src = "-module(rabbit_net).\n-compile({parse_transform, lager_transform}).\n-export_type([socket/0]).\n";
    assert!(!extract_exported_types(src).complete);
}

#[test]
fn a_macro_entry_in_an_export_type_list_makes_the_set_incomplete() {
    let src = "-module(rabbit_net).\n-export_type([socket/0, ?EXTRA_TYPES]).\n";
    let set = extract_exported_types(src);
    assert!(!set.complete);
    // the readable entry is still read: the caller decides to withhold
    assert_eq!(set.types.len(), 1);
}

#[test]
fn a_module_with_no_export_type_yields_an_empty_complete_set() {
    let set = extract_exported_types("-module(rabbit_net).\n-export([peername/1]).\n");
    assert!(set.complete);
    assert!(set.types.is_empty());
}

#[test]
fn type_opaque_and_nominal_declarations_all_count_as_definitions() {
    let src = "-module(rabbit_net).\n\
               -type socket() :: inet:socket().\n\
               -opaque proxy_socket() :: {rabbit_proxy_socket, inet:socket(), term()}.\n\
               -nominal hostname() :: inet:hostname().\n";
    let defined = extract_defined_types(src);
    assert!(defined.contains(&key("socket", 0)));
    assert!(defined.contains(&key("proxy_socket", 0)));
    assert!(defined.contains(&key("hostname", 0)));
}

#[test]
fn a_parameterised_type_declaration_carries_its_arity() {
    let src = "-module(rabbit_net).\n-type result(Ok, Err) :: {ok, Ok} | {error, Err}.\n";
    let defined = extract_defined_types(src);
    assert!(defined.contains(&key("result", 2)));
    assert!(!defined.contains(&key("result", 0)));
}

#[test]
fn a_spec_is_not_a_type_declaration() {
    let src = "-module(rabbit_net).\n-spec peername(socket()) -> {ok, term()}.\n";
    assert!(extract_defined_types(src).is_empty());
}

#[test]
fn a_type_declaration_without_the_double_colon_is_not_read() {
    // -type without :: does not compile
    let src = "-module(rabbit_net).\n-type socket().\n";
    assert!(extract_defined_types(src).is_empty());
}
