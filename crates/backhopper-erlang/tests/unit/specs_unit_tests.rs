// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang::specs::{parse_callable_signature, parse_type_decl};

// parse_callable_signature is defined and tested in backhopper-erlang-scan; this exercises the re-export
#[test]
fn callable_signature_reexport_stays_stable() {
    let s = parse_callable_signature("register(X) -> ok").unwrap();
    assert_eq!(s.name, "register");
    assert_eq!(s.arity, 1);
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
