// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::names::{Arity, FunctionName};
use backhopper_core::model::snapshot::FunArity;
use backhopper_erlang::clause_heads::extract;

fn fa(name: &str, arity: u8) -> FunArity {
    FunArity {
        name: FunctionName::from_str(name).unwrap(),
        arity: Arity::new(arity),
    }
}

#[test]
fn single_clause_zero_arity() {
    let src = "-module(ra).\nhello() -> ok.\n";
    let out = extract(src);
    let heads = out.get(&fa("hello", 0)).expect("hello/0");
    assert_eq!(heads.len(), 1);
    assert!(heads[0].is_empty());
}

#[test]
fn single_clause_with_variable_arg() {
    let src = "-module(ra_server).\ngreet(Name) -> Name.\n";
    let out = extract(src);
    let heads = out.get(&fa("greet", 1)).expect("greet/1");
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0], vec![ArgShape::Variable]);
}

#[test]
fn multiple_clauses_collected_for_same_function() {
    let src = "\
-module(rabbit_fifo).
which_module(0) -> rabbit_fifo_v0;
which_module(1) -> rabbit_fifo_v1;
which_module(2) -> rabbit_fifo_v3.
";
    let out = extract(src);
    let heads = out.get(&fa("which_module", 1)).expect("which_module/1");
    assert_eq!(heads.len(), 3);
    for c in heads {
        assert_eq!(c.len(), 1);
        assert_eq!(c[0], ArgShape::Integer);
    }
}

#[test]
fn tagged_tuple_arg_recognised_as_tuple_size() {
    let src = "-module(khepri).\nhandle({ok, X}) -> X.\n";
    let out = extract(src);
    let heads = out.get(&fa("handle", 1)).expect("handle/1");
    assert_eq!(heads[0], vec![ArgShape::Tuple { size: 2 }]);
}

#[test]
fn atom_argument_collapses_to_atom_shape() {
    let src = "-module(ra_machine).\nflag(true) -> 1.\n";
    let out = extract(src);
    let heads = out.get(&fa("flag", 1)).expect("flag/1");
    assert_eq!(
        heads[0],
        vec![ArgShape::Atom {
            name: "true".into()
        }]
    );
}

#[test]
fn when_guard_does_not_split_the_clause_head() {
    let src = "-module(ra_lib).\ndump(X) when is_integer(X) -> X.\n";
    let out = extract(src);
    let heads = out.get(&fa("dump", 1)).expect("dump/1");
    assert_eq!(heads[0], vec![ArgShape::Variable]);
}

#[test]
fn when_guard_wrapped_onto_the_next_line_is_still_a_clause() {
    let src = "-module(ra_lib).\nceiling(N)\n  when is_integer(N) -> N.\n";
    let out = extract(src);
    let heads = out.get(&fa("ceiling", 1)).expect("ceiling/1");
    assert_eq!(heads[0], vec![ArgShape::Variable]);
}

#[test]
fn semicolon_inside_body_does_not_create_phantom_clause() {
    let src = "\
-module(osiris_log).
id(X) ->
    case X of
        0 -> zero;
        _ -> nonzero
    end.
";
    let out = extract(src);
    let heads = out.get(&fa("id", 1)).expect("id/1");
    assert_eq!(heads.len(), 1, "case-branch ; should not split clauses");
}

#[test]
fn ignores_attributes_at_column_zero() {
    let src = "-module(ra_directory).\n-export([hello/0]).\nhello() -> ok.\n";
    let out = extract(src);
    assert!(out.contains_key(&fa("hello", 0)));
    assert!(
        !out.contains_key(&fa("export", 1)),
        "attribute is not a clause"
    );
}

#[test]
fn ignores_comments() {
    let src =
        "%% top-level comment\n-module(khepri_machine).\n%% another comment\nis_empty() -> ok.\n";
    let out = extract(src);
    assert!(out.contains_key(&fa("is_empty", 0)));
}

#[test]
fn record_argument_recognised() {
    let src = "-module(ra_log).\nhandle(#state{}) -> ok.\n";
    let out = extract(src);
    let heads = out.get(&fa("handle", 1)).expect("handle/1");
    match &heads[0][0] {
        ArgShape::Record { name } => assert_eq!(name.as_str(), "state"),
        other => panic!("expected Record, got {other:?}"),
    }
}

#[test]
fn multi_arity_distinguished() {
    let src = "\
-module(seshat).
fetch() -> ok.
fetch(X) -> X.
fetch(X, Y) -> {X, Y}.
";
    let out = extract(src);
    assert!(out.contains_key(&fa("fetch", 0)));
    assert!(out.contains_key(&fa("fetch", 1)));
    assert!(out.contains_key(&fa("fetch", 2)));
}

#[test]
fn exported_function_with_only_export_is_not_in_clause_heads() {
    let src = "-module(osiris).\n-export([missing/1]).\n";
    let out = extract(src);
    assert!(!out.contains_key(&fa("missing", 1)));
}

// a $" char literal in a clause body must not open a string that runs to end of file
#[test]
fn char_literal_delimiter_in_body_does_not_eat_later_clauses() {
    let src = "\
-module(rabbit_db_queue).
first(X) ->
    Y = $\",
    {X, Y};
first(_) -> other.
second(Z) -> Z.
";
    let out = extract(src);
    assert!(out.contains_key(&fa("first", 1)));
    assert!(out.contains_key(&fa("second", 1)));
}

// a $( char literal in a clause guard must not unbalance the head parens
#[test]
fn char_literal_paren_in_guard_keeps_arity() {
    let src =
        "-module(rabbit_db_binding).\nclassify(C) when C =:= $( -> open.\nclassify(_) -> other.\n";
    let out = extract(src);
    assert!(out.contains_key(&fa("classify", 1)));
}

// from ra_server: multi-line argument list and guard with map and record
// patterns; the head parser must find each clause arrow across newlines
#[test]
fn multi_line_head_and_guard_from_ra_server() {
    let src = "\
-module(ra_server).
handle_leader({PeerId, #append_entries_reply{term = Term}},
              #{current_term := CurTerm,
                cfg := #cfg{log_id = LogId}} = State0)
  when Term > CurTerm ->
    {follower, update_term(Term, State0), []};
handle_leader({PeerId, #append_entries_reply{success = false}},
              State0 = #{cluster := Nodes})
  when not is_map_key(PeerId, Nodes) ->
    {leader, State0, []}.
";
    let out = extract(src);
    let heads = out.get(&fa("handle_leader", 2)).expect("handle_leader/2");
    assert_eq!(heads.len(), 2);
}

// a semicolon inside a body comment is not a clause separator: the body
// scanner must skip to end of line
#[test]
fn semicolon_in_body_comment_does_not_split_clause() {
    let src = "\
-module(ra_log).
append(Entry, State) ->
    %% write the entry; then continue
    write(Entry, State);
append([], State) ->
    State.
";
    let out = extract(src);
    let heads = out.get(&fa("append", 2)).expect("append/2");
    assert_eq!(heads.len(), 2);
}

// a $( char literal in the argument list is data, not an opening paren: arity stays 2
#[test]
fn char_literal_open_paren_in_args_keeps_arity() {
    let src = "-module(rabbit_binary_parser).\nclassify($(, Rest) -> {paren, Rest}.\n";
    let out = extract(src);
    assert!(out.contains_key(&fa("classify", 2)));
}

// a $; char literal is not a clause separator and a trailing $. is not a
// form terminator: both clauses of format/1 are kept
#[test]
fn char_literal_semicolon_and_dot_in_body_are_data() {
    let src = "\
-module(rabbit_misc).
format(X) ->
    Sep = $;,
    join(X, Sep);
format([]) ->
    $..
";
    let out = extract(src);
    assert_eq!(out.get(&fa("format", 1)).map(|v| v.len()), Some(2));
}

// the arrow search must track {} and [] depth in a multi-line guard so
// the -> after the guard is the one that ends the head
#[test]
fn multi_line_guard_with_tuple_and_list_brackets() {
    let src = "\
-module(ra_machine).
apply(Meta, Cmd, State)
  when Cmd =:= {down, normal}, element(1, State) =/= [] ->
    {State, ok};
apply(_Meta, _Cmd, State) ->
    {State, error}.
";
    let out = extract(src);
    let heads = out.get(&fa("apply", 3)).expect("apply/3");
    assert_eq!(heads.len(), 2);
}

#[test]
fn triple_quoted_body_does_not_swallow_the_following_heads() {
    let src = r#"log_summary(Ra) ->
    Doc = """
    Prose with "quotes" and sentences. Ending with dots.
    -export([not_an_attribute/0]).
    """,
    Doc.
add_member(Cluster, Member) -> {ok, Member, Cluster}.
remove_member(Cluster, Member, Timeout) -> {Cluster, Member, Timeout}.
"#;
    let heads = extract(src);
    let keys: Vec<FunArity> = heads.keys().cloned().collect();
    assert_eq!(
        keys,
        vec![
            fa("add_member", 2),
            fa("log_summary", 1),
            fa("remove_member", 3),
        ]
    );
}

#[test]
fn sigil_body_does_not_swallow_the_following_heads() {
    let src = r#"banner(Node) ->
    ~s(quotes " and arrows -> and dots. here),
    Node.
tick(State) -> State.
"#;
    let heads = extract(src);
    let keys: Vec<FunArity> = heads.keys().cloned().collect();
    assert_eq!(keys, vec![fa("banner", 1), fa("tick", 1)]);
}
