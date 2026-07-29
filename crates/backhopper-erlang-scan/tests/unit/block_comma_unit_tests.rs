// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Commas inside `end`-closed block expressions are statement or
//! clause separators, not argument separators. The shapes here come
//! from real RabbitMQ test suites, where an inline `fun` passed to
//! `lists:foreach` carries its own statement commas.

use backhopper_erlang_scan::{ScanArity, arity_of_args, scan_arity, split_top_level_args};

fn exact(input: &str) -> u8 {
    match scan_arity(input) {
        ScanArity::Exact(n) => n,
        ScanArity::Unterminated => panic!("unterminated: {input}"),
    }
}

#[test]
fn a_fun_body_comma_is_not_an_argument_separator() {
    let args = "\n  fun({Name, Val}) ->\n      C = connect(atom_to_binary(Name), Config),\n      ok = emqtt:publish(C, Topic, #{Name => Val}),\n      util:await_exit(C)\n  end, NotApplicable),";
    assert_eq!(exact(args), 2);
}

#[test]
fn a_named_fun_body_is_inert() {
    assert_eq!(exact("fun Loop(N) -> drain(N), Loop(N - 1) end, Queue)"), 2);
}

#[test]
fn a_guarded_fun_body_is_inert() {
    assert_eq!(
        exact("fun(X) when X > 0 -> ack(X), settle(X) end, Deliveries)"),
        2
    );
}

#[test]
fn a_multi_clause_fun_body_is_inert() {
    assert_eq!(
        exact("fun(classic) -> ok; (quorum) -> drop(1, 2) end, Types)"),
        2
    );
}

#[test]
fn a_fun_reference_opens_no_block() {
    assert_eq!(exact("fun rabbit_net:fast_close/1, Sockets, Timeout)"), 3);
    assert_eq!(exact("fun close/1, Sockets)"), 2);
    assert_eq!(exact("fun M:F/A, Args)"), 2);
}

#[test]
fn a_fun_type_in_a_spec_opens_no_block() {
    // the type forms close with no `end`: the -spec argument list must keep its arity
    assert_eq!(
        arity_of_args("fun((rabbit_types:amqqueue()) -> ok), map()"),
        2
    );
    assert_eq!(arity_of_args("fun(), pos_integer()"), 2);
}

#[test]
fn nested_blocks_balance() {
    let args = "fun(Q) -> case Q of classic -> a(1, 2); quorum -> b(3, 4) end end, Queues)";
    assert_eq!(exact(args), 2);
}

#[test]
fn a_try_block_argument_is_inert() {
    let args = "try connect(H, P), handshake(H) catch _:_ -> down, retry end, Hosts)";
    assert_eq!(exact(args), 2);
}

#[test]
fn a_receive_block_argument_is_inert() {
    assert_eq!(
        exact("receive {ack, T} -> T, ok after 500 -> late end, D)"),
        2
    );
}

#[test]
fn an_end_in_an_identifier_does_not_decrement() {
    assert_eq!(exact("send_end(A, B), End, Tail)"), 3);
}

#[test]
fn a_bare_maybe_atom_keeps_the_argument_count() {
    // `maybe` is not reserved, so a bare atom in argument position is
    // legal and must not open a block
    assert_eq!(exact("maybe, exclusive)"), 2);
}

#[test]
fn a_keyword_in_a_string_is_inert() {
    assert_eq!(exact("\"fun end case\", Mode)"), 2);
}

#[test]
fn a_keyword_in_a_quoted_atom_is_inert() {
    assert_eq!(exact("'end', 'fun', Mode)"), 3);
}

#[test]
fn splitting_agrees_with_counting_on_block_arguments() {
    let inner = "fun(X) -> a(X), b(X) end, Acc0, List";
    assert_eq!(split_top_level_args(inner).len(), 3);
    assert_eq!(arity_of_args(inner), 3);
}

#[test]
fn a_missing_end_never_terminates_the_argument_list() {
    // a close paren inside an unclosed block is not the call's close
    assert_eq!(
        scan_arity("fun(X) -> a(X), b(X), L)"),
        ScanArity::Unterminated
    );
}
