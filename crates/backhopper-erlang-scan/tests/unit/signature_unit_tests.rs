// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang_scan::parse_callable_signature;

#[test]
fn parses_simple_spec() {
    let s = parse_callable_signature("register(X) -> ok").unwrap();
    assert_eq!(s.name, "register");
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
fn bitstring_type_arg_does_not_inflate_arity() {
    // `<<_:8, _:_*8>>` is one argument; its inner comma belongs to the
    // bitstring type, not the argument list.
    let s = parse_callable_signature("encode(<<_:8, _:_*8>>) -> binary()").unwrap();
    assert_eq!(s.name, "encode");
    assert_eq!(s.arity, 1);
}

#[test]
fn bitstring_type_among_other_args_counts_correctly() {
    let s =
        parse_callable_signature("write(Fd :: file:io_device(), <<_:8, _:_*8>>) -> ok").unwrap();
    assert_eq!(s.arity, 2);
}

#[test]
fn signature_keeps_the_arrow_and_return_type() {
    let s = parse_callable_signature("segment_entry_count() -> pos_integer()").unwrap();
    assert_eq!(s.signature, "segment_entry_count() -> pos_integer()");
}

#[test]
fn module_qualified_form_is_not_recognised() {
    assert_eq!(
        parse_callable_signature("rabbit_misc:format(Fmt, Args) -> string()"),
        None
    );
}

#[test]
fn missing_arrow_is_not_recognised() {
    assert_eq!(parse_callable_signature("info(state())"), None);
}
