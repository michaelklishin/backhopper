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

#[test]
fn a_maybe_block_argument_counts_as_one() {
    assert_eq!(
        exact("maybe ok ?= khepri:put(Id, V), V else _ -> err end, Fallback)"),
        2
    );
}

#[test]
fn maybe_blocks_nest_with_other_blocks() {
    assert_eq!(exact("case X of _ -> maybe A = f(X), A end end, Y)"), 2);
    assert_eq!(
        exact("maybe case f(X) of ok -> ok end else _ -> err end, Y)"),
        2
    );
}

#[test]
fn a_bare_maybe_atom_keeps_the_argument_count() {
    assert_eq!(exact("maybe, x)"), 2);
    assert_eq!(exact("f(maybe), x)"), 2);
    assert_eq!(exact("{maybe, on}, x)"), 2);
}

#[test]
fn atom_maybe_before_an_arrow_never_opens_a_phantom_block() {
    assert_eq!(exact("case get() of maybe -> a end, Y)"), 2);
}

#[test]
fn atom_maybe_before_a_semicolon_stays_inert() {
    assert_eq!(exact("fun(x) -> maybe; (y) -> ok end, Z)"), 2);
}

#[test]
fn atom_maybe_before_a_continuation_word_stays_inert() {
    assert_eq!(exact("X =:= maybe orelse Y, Z)"), 2);
    assert_eq!(exact("V == maybe andalso W, Z)"), 2);
}

#[test]
fn the_accepted_misreads_degrade_to_the_old_reading() {
    // a block whose first expression is parenthesized reads as a call
    // to an atom named maybe: depth short by one, never a phantom block
    assert_eq!(scan_arity("maybe (A + B), x end, Y)"), ScanArity::Exact(3));
    assert_eq!(exact("maybe ?LOG(X), ok end, Y)"), 3);
}

#[test]
fn record_access_on_atom_maybe_keeps_the_count() {
    assert_eq!(exact("maybe#state.field, Y)"), 2);
    assert_eq!(exact("maybe #state{f = 1}, Y)"), 2);
}

#[test]
fn a_truncated_slice_ending_in_maybe_opens_nothing() {
    assert_eq!(split_top_level_args("A, maybe").len(), 2);
}

#[test]
fn a_reserved_word_record_name_is_not_a_block_token() {
    assert_eq!(exact("#end{a = 1, b = 2}, Y)"), 2);
    assert_eq!(exact("#case{f = g(1), h = 2}, Y)"), 2);
    assert_eq!(exact("case f(X) of _ -> #end{a = 1} end, Y)"), 2);
}

#[test]
fn a_comment_inside_an_attribute_body_never_feeds_the_block_depth() {
    // an export list annotated with `% maybe` must keep every entry
    // after the comment
    let entries = backhopper_erlang_scan::split_top_level_commas(
        "ignore/2,\n% maybe\niter_maybe/2,\n% coercion\nto_list/1,\ncons/2",
    );
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[3], "cons/2");
}

#[test]
fn a_comment_holding_a_block_keyword_stays_inert_in_arity_reads() {
    assert_eq!(exact("A, % case of doom\nB)"), 2);
}

#[test]
fn a_quoted_record_name_beside_block_keywords_stays_inert() {
    assert_eq!(exact("case f(X) of _ -> #'end'{a = 1} end, Y)"), 2);
}

#[test]
fn a_maybe_block_opening_with_a_record_literal_degrades_to_the_old_reading() {
    // the `#` follower keeps the atom reading: depth short by one, the
    // block's own `end` absorbs the difference, neighbors keep counting
    assert_eq!(exact("maybe #state{f = 1} = g(X), ok end, Y)"), 3);
}

#[test]
fn a_comment_between_maybe_and_its_body_keeps_the_block_reading() {
    assert_eq!(exact("maybe % note\n X = f(), Y = g(), ok end, B)"), 2);
}

#[test]
fn a_comment_between_fun_and_its_clause_keeps_the_block_reading() {
    assert_eq!(exact("fun % note\n (X) -> a(X), b(X) end, B)"), 2);
}

#[test]
fn comment_removal_keeps_multibyte_chars_whole() {
    use backhopper_erlang_scan::remove_line_comments;
    assert_eq!(remove_line_comments("f % note\n= $ä"), "f \n= $ä");
    assert_eq!(remove_line_comments("% c\n é"), "\n é");
    assert_eq!(remove_line_comments("\"a%b\", x"), "\"a%b\", x");
}
