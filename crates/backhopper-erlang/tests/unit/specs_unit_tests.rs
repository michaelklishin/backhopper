// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang::specs::{parse_callable_signature, parse_type_decl};

#[test]
fn parses_simple_spec() {
    let s = parse_callable_signature("foo(X) -> ok").unwrap();
    assert_eq!(s.name, "foo");
    assert_eq!(s.arity, 1);
}

#[test]
fn parses_zero_arity() {
    let s = parse_callable_signature("init() -> state()").unwrap();
    assert_eq!(s.arity, 0);
}

#[test]
fn parses_two_arg_with_typed_args() {
    let s = parse_callable_signature(
        "process_command(Id :: term(), Cmd :: term()) -> ok | {error, term()}",
    )
    .unwrap();
    assert_eq!(s.arity, 2);
}

#[test]
fn parses_type_declaration() {
    let (name, arity, rhs) =
        parse_type_decl("range() :: undefined | {ra:index(), ra:index()}").unwrap();
    assert_eq!(name, "range");
    assert_eq!(arity, 0);
    assert!(rhs.contains("undefined"));
}

#[test]
fn parses_parametrized_type() {
    let (name, arity, _) = parse_type_decl("either(A, B) :: A | B").unwrap();
    assert_eq!(name, "either");
    assert_eq!(arity, 2);
}