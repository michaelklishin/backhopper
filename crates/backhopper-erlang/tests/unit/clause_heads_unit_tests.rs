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
    let src = "-module(m).\nhello() -> ok.\n";
    let out = extract(src);
    let heads = out.get(&fa("hello", 0)).expect("hello/0");
    assert_eq!(heads.len(), 1);
    assert!(heads[0].is_empty());
}

#[test]
fn single_clause_with_variable_arg() {
    let src = "-module(m).\ngreet(Name) -> Name.\n";
    let out = extract(src);
    let heads = out.get(&fa("greet", 1)).expect("greet/1");
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0], vec![ArgShape::Variable]);
}

#[test]
fn multiple_clauses_collected_for_same_function() {
    let src = "\
-module(m).
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
    let src = "-module(m).\nhandle({ok, X}) -> X.\n";
    let out = extract(src);
    let heads = out.get(&fa("handle", 1)).expect("handle/1");
    assert_eq!(heads[0], vec![ArgShape::Tuple { size: 2 }]);
}

#[test]
fn atom_argument_collapses_to_atom_shape() {
    let src = "-module(m).\nflag(true) -> 1.\n";
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
    let src = "-module(m).\nf(X) when is_integer(X) -> X.\n";
    let out = extract(src);
    let heads = out.get(&fa("f", 1)).expect("f/1");
    assert_eq!(heads[0], vec![ArgShape::Variable]);
}

#[test]
fn semicolon_inside_body_does_not_create_phantom_clause() {
    let src = "\
-module(m).
f(X) ->
    case X of
        0 -> zero;
        _ -> nonzero
    end.
";
    let out = extract(src);
    let heads = out.get(&fa("f", 1)).expect("f/1");
    assert_eq!(heads.len(), 1, "case-branch ; should not split clauses");
}

#[test]
fn ignores_attributes_at_column_zero() {
    let src = "-module(m).\n-export([hello/0]).\nhello() -> ok.\n";
    let out = extract(src);
    assert!(out.contains_key(&fa("hello", 0)));
    assert!(
        !out.contains_key(&fa("export", 1)),
        "attribute is not a clause"
    );
}

#[test]
fn ignores_comments() {
    let src = "%% top-level comment\n-module(m).\n%% another comment\nfoo() -> ok.\n";
    let out = extract(src);
    assert!(out.contains_key(&fa("foo", 0)));
}

#[test]
fn record_argument_recognised() {
    let src = "-module(m).\nhandle(#state{}) -> ok.\n";
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
-module(m).
foo() -> ok.
foo(X) -> X.
foo(X, Y) -> {X, Y}.
";
    let out = extract(src);
    assert!(out.contains_key(&fa("foo", 0)));
    assert!(out.contains_key(&fa("foo", 1)));
    assert!(out.contains_key(&fa("foo", 2)));
}

#[test]
fn exported_function_with_only_export_is_not_in_clause_heads() {
    let src = "-module(m).\n-export([missing/1]).\n";
    let out = extract(src);
    assert!(!out.contains_key(&fa("missing", 1)));
}
