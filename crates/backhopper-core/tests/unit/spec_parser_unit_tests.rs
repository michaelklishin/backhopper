// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::spec_ast::SpecType;
use backhopper_core::model::spec_parser::{parse, parse_signature_return};

fn assert_atom(t: SpecType, name: &str) {
    match t {
        SpecType::Atom { name: n } => assert_eq!(n, name),
        other => panic!("expected Atom({name:?}), got {other:?}"),
    }
}

#[test]
fn parse_bare_atom() {
    assert_atom(parse("ok"), "ok");
    assert_atom(parse("'with space'"), "with space");
}

#[test]
fn parse_builtin_call() {
    match parse("atom()") {
        SpecType::Builtin { name } => assert_eq!(name, "atom"),
        other => panic!("expected Builtin(atom), got {other:?}"),
    }
    match parse("non_neg_integer()") {
        SpecType::Builtin { name } => assert_eq!(name, "non_neg_integer"),
        other => panic!("expected Builtin(non_neg_integer), got {other:?}"),
    }
}

#[test]
fn parse_tagged_tuple_two_arity() {
    match parse("{ok, X}") {
        SpecType::TaggedTuple { tag, arity } => {
            assert_eq!(tag, "ok");
            assert_eq!(arity, 2);
        }
        other => panic!("expected TaggedTuple, got {other:?}"),
    }
}

#[test]
fn parse_tagged_tuple_three_arity() {
    match parse("{open, response_code(), #{}}") {
        SpecType::TaggedTuple { tag, arity } => {
            assert_eq!(tag, "open");
            assert_eq!(arity, 3);
        }
        other => panic!("expected TaggedTuple, got {other:?}"),
    }
}

#[test]
fn parse_untagged_tuple() {
    match parse("{X, Y}") {
        SpecType::Tuple { arity } => assert_eq!(arity, 2),
        other => panic!("expected Tuple, got {other:?}"),
    }
}

#[test]
fn parse_empty_tuple() {
    match parse("{}") {
        SpecType::Tuple { arity } => assert_eq!(arity, 0),
        other => panic!("expected Tuple(0), got {other:?}"),
    }
}

#[test]
fn parse_union_of_tagged_tuples() {
    let t = parse("{ok, X} | {error, R}");
    match t {
        SpecType::Union { variants } => {
            assert_eq!(variants.len(), 2);
            let has_ok = variants
                .iter()
                .any(|v| matches!(v, SpecType::TaggedTuple { tag, .. } if tag == "ok"));
            let has_error = variants
                .iter()
                .any(|v| matches!(v, SpecType::TaggedTuple { tag, .. } if tag == "error"));
            assert!(has_ok && has_error);
        }
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn parse_union_with_bare_atom() {
    let t = parse("ok | {error, R}");
    match t {
        SpecType::Union { variants } => {
            assert_eq!(variants.len(), 2);
            assert!(
                variants
                    .iter()
                    .any(|v| matches!(v, SpecType::Atom { name } if name == "ok"))
            );
        }
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn union_canonicalises_duplicates() {
    let t = parse("ok | ok | {error, R} | ok");
    match t {
        SpecType::Union { variants } => {
            let oks = variants
                .iter()
                .filter(|v| matches!(v, SpecType::Atom { name } if name == "ok"))
                .count();
            assert_eq!(oks, 1);
        }
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn parse_list_element() {
    match parse("[binary()]") {
        SpecType::List { element } => match *element {
            SpecType::Builtin { name } => assert_eq!(name, "binary"),
            other => panic!("expected Builtin element, got {other:?}"),
        },
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parse_empty_list() {
    match parse("[]") {
        SpecType::List { .. } => {}
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parse_map_collapses_entries() {
    match parse("#{atom() => binary()}") {
        SpecType::Map => {}
        other => panic!("expected Map, got {other:?}"),
    }
}

#[test]
fn parse_record() {
    match parse("#user{}") {
        SpecType::Record { name } => assert_eq!(name, "user"),
        other => panic!("expected Record, got {other:?}"),
    }
    match parse("#user{name :: binary()}") {
        SpecType::Record { name } => assert_eq!(name, "user"),
        other => panic!("expected Record (with fields), got {other:?}"),
    }
}

#[test]
fn parse_type_call_with_module() {
    match parse("ra:index()") {
        SpecType::TypeCall {
            module,
            name,
            arity,
        } => {
            assert_eq!(module.as_deref(), Some("ra"));
            assert_eq!(name, "index");
            assert_eq!(arity, 0);
        }
        other => panic!("expected TypeCall, got {other:?}"),
    }
}

#[test]
fn parse_local_type_call() {
    match parse("state()") {
        SpecType::TypeCall {
            module,
            name,
            arity,
        } => {
            assert_eq!(module, None);
            assert_eq!(name, "state");
            assert_eq!(arity, 0);
        }
        other => panic!("expected TypeCall, got {other:?}"),
    }
}

#[test]
fn parse_fun_type() {
    match parse("fun((integer()) -> ok)") {
        SpecType::Fun => {}
        other => panic!("expected Fun, got {other:?}"),
    }
}

#[test]
fn parse_integer_literal() {
    match parse("42") {
        SpecType::Integer { value } => assert_eq!(value, 42),
        other => panic!("expected Integer, got {other:?}"),
    }
    match parse("-7") {
        SpecType::Integer { value } => assert_eq!(value, -7),
        other => panic!("expected Integer, got {other:?}"),
    }
}

#[test]
fn parse_var_uppercase() {
    match parse("State") {
        SpecType::Var { name } => assert_eq!(name, "State"),
        other => panic!("expected Var, got {other:?}"),
    }
}

#[test]
fn parse_underscore_var() {
    match parse("_") {
        SpecType::Var { name } => assert_eq!(name, "_"),
        other => panic!("expected Var(_), got {other:?}"),
    }
}

#[test]
fn parse_signature_return_strips_arrow_and_args() {
    let t = parse_signature_return("process(A, B) -> {ok, B}");
    match t {
        SpecType::TaggedTuple { tag, arity } => {
            assert_eq!(tag, "ok");
            assert_eq!(arity, 2);
        }
        other => panic!("expected TaggedTuple(ok, 2), got {other:?}"),
    }
}

#[test]
fn parse_signature_return_skips_when_guard() {
    let t = parse_signature_return("cast(B) -> B when B :: binary()");
    match t {
        SpecType::Var { name } => assert_eq!(name, "B"),
        other => panic!("expected Var(B), got {other:?}"),
    }
}

#[test]
fn parse_signature_return_handles_no_arrow() {
    let t = parse_signature_return("machine_version");
    assert!(matches!(t, SpecType::Unknown));
}

#[test]
fn matches_handles_unknown_as_compatible() {
    let a = SpecType::Unknown;
    let b = SpecType::TaggedTuple {
        tag: "ok".into(),
        arity: 2,
    };
    assert!(a.matches(&b));
    assert!(b.matches(&a));
}

#[test]
fn matches_distinguishes_tagged_tuple_arity() {
    let a = SpecType::TaggedTuple {
        tag: "open".into(),
        arity: 2,
    };
    let b = SpecType::TaggedTuple {
        tag: "open".into(),
        arity: 3,
    };
    assert!(!a.matches(&b));
}

#[test]
fn matches_unions_field_against_member() {
    let a = SpecType::Union {
        variants: vec![
            SpecType::TaggedTuple {
                tag: "ok".into(),
                arity: 2,
            },
            SpecType::TaggedTuple {
                tag: "error".into(),
                arity: 2,
            },
        ],
    };
    let b = SpecType::TaggedTuple {
        tag: "ok".into(),
        arity: 2,
    };
    assert!(a.matches(&b));
    assert!(b.matches(&a));
}

#[test]
fn matches_handles_integer_against_builtin() {
    let a = SpecType::Integer { value: 5 };
    let b = SpecType::Builtin {
        name: "pos_integer".into(),
    };
    assert!(a.matches(&b));
    assert!(b.matches(&a));
}

#[test]
fn matches_rejects_unrelated_atoms() {
    let a = SpecType::Atom { name: "ok".into() };
    let b = SpecType::Atom {
        name: "error".into(),
    };
    assert!(!a.matches(&b));
}

#[test]
fn parse_nested_union_in_tuple() {
    let t = parse("{ok, integer() | binary()}");
    match t {
        SpecType::TaggedTuple { tag, arity } => {
            assert_eq!(tag, "ok");
            assert_eq!(arity, 2);
        }
        other => panic!("expected TaggedTuple, got {other:?}"),
    }
}

#[test]
fn parse_real_world_callback_return_dedups_same_shape() {
    // {error, ...} and {error, timeout} share tag and arity: the canonicaliser collapses them.
    let return_text = "{ok, pid()} | {error, {already_started, pid()}} | {error, timeout}";
    let t = parse(return_text);
    match t {
        SpecType::Union { variants } => {
            assert_eq!(variants.len(), 2);
            let kinds: Vec<&str> = variants
                .iter()
                .filter_map(|v| match v {
                    SpecType::TaggedTuple { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(kinds.contains(&"ok"));
            assert!(kinds.contains(&"error"));
        }
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn parse_range_at_top_level() {
    match parse("1..255") {
        SpecType::Range { low, high } => {
            assert_eq!(low, 1);
            assert_eq!(high, 255);
        }
        other => panic!("expected Range, got {other:?}"),
    }
}

#[test]
fn a_range_inside_a_tuple_no_longer_poisons_the_tuple() {
    match parse("{frame, 1..255}") {
        SpecType::TaggedTuple { tag, arity } => {
            assert_eq!(tag, "frame");
            assert_eq!(arity, 2);
        }
        other => panic!("expected TaggedTuple, got {other:?}"),
    }
}

#[test]
fn parse_range_inside_a_union() {
    match parse("undefined | 1..16") {
        SpecType::Union { variants } => {
            assert!(
                variants
                    .iter()
                    .any(|v| matches!(v, SpecType::Range { low: 1, high: 16 }))
            );
        }
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn parse_binary_size_specs() {
    assert!(matches!(parse("<<>>"), SpecType::Binary));
    assert!(matches!(parse("<<_:8>>"), SpecType::Binary));
    assert!(matches!(parse("<<_:_*8>>"), SpecType::Binary));
}

#[test]
fn an_annotated_return_reads_as_its_type() {
    match parse("Result :: ok | error") {
        SpecType::Union { variants } => assert_eq!(variants.len(), 2),
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn an_annotated_argument_reads_as_its_type() {
    let t = parse_signature_return("info(Name :: atom()) -> Value :: binary()");
    match t {
        SpecType::Builtin { name } => assert_eq!(name, "binary"),
        other => panic!("expected Builtin(binary), got {other:?}"),
    }
}

#[test]
fn parse_based_and_underscored_integers_and_char_literals() {
    assert!(matches!(parse("16#FF"), SpecType::Integer { value: 255 }));
    assert!(matches!(parse("1_000"), SpecType::Integer { value: 1000 }));
    assert!(matches!(parse("$a"), SpecType::Integer { value: 97 }));
    assert!(matches!(
        parse("$a..$z"),
        SpecType::Range { low: 97, high: 122 }
    ));
}

#[test]
fn each_new_builtin_name_parses_as_builtin() {
    for name in [
        "dynamic",
        "nonempty_binary",
        "nonempty_bitstring",
        "nonempty_improper_list",
        "nonempty_maybe_improper_list",
    ] {
        match parse(&format!("{name}()")) {
            SpecType::Builtin { name: n } => assert_eq!(n, name),
            other => panic!("expected Builtin({name}), got {other:?}"),
        }
    }
}

#[test]
fn dynamic_matches_any_shape() {
    let dynamic = SpecType::Builtin {
        name: "dynamic".into(),
    };
    assert!(dynamic.matches(&parse("integer()")));
    assert!(dynamic.matches(&parse("binary()")));
    assert!(dynamic.matches(&parse("#ra_server_state{}")));
    assert!(parse("#ra_server_state{}").matches(&dynamic));
}

#[test]
fn range_matches_ranges_and_integer_builtins() {
    let range = parse("1..255");
    assert!(range.matches(&parse("1..255")));
    assert!(!range.matches(&parse("1..16")));
    assert!(range.matches(&parse("integer()")));
    assert!(range.matches(&parse("non_neg_integer()")));
    assert!(range.matches(&parse("number()")));
    assert!(!range.matches(&parse("binary()")));
}

#[test]
fn binary_matches_binary_and_bitstring_builtins() {
    let bin = parse("<<_:8>>");
    assert!(bin.matches(&parse("<<>>")));
    assert!(bin.matches(&parse("binary()")));
    assert!(bin.matches(&parse("bitstring()")));
    assert!(!bin.matches(&parse("integer()")));
}

#[test]
fn char_literal_range_survives_the_arrow_search() {
    // `$-` and `$>` must not read as part of an arrow, and `$(` must not
    // unbalance the depth the arrow search keeps
    assert!(matches!(
        parse_signature_return("classify(char()) -> $a..$z"),
        SpecType::Range { low: 97, high: 122 }
    ));
    assert!(matches!(
        parse_signature_return("open_paren() -> $("),
        SpecType::Integer { value: 40 }
    ));
}

#[test]
fn char_literal_quote_does_not_open_a_string_before_the_arrow() {
    assert!(matches!(
        parse_signature_return("quote_char() -> $\" | eof"),
        SpecType::Union { .. }
    ));
}

#[test]
fn char_literal_comma_inside_a_tuple_parses_as_its_code_point() {
    let t = parse("{sep, $,}");
    match t {
        SpecType::TaggedTuple { tag, arity } => {
            assert_eq!(tag, "sep");
            assert_eq!(arity, 2);
        }
        other => panic!("expected tagged tuple, got {other:?}"),
    }
}
