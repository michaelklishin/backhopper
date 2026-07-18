// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `SpecType` canonicalisation and the return-shape `matches` relation.
//! Shapes are drawn from real ra spec returns: `{State, reply()}`,
//! `non_neg_integer()` versions, `ra:index()` type calls, union returns.

use backhopper_core::model::spec_ast::SpecType;

fn atom(n: &str) -> SpecType {
    SpecType::Atom { name: n.into() }
}

fn builtin(n: &str) -> SpecType {
    SpecType::Builtin { name: n.into() }
}

#[test]
fn canonicalise_empty_union_becomes_unknown() {
    let u = SpecType::Union {
        variants: Vec::new(),
    };
    assert!(matches!(u.canonicalise(), SpecType::Unknown));
}

#[test]
fn canonicalise_single_variant_union_collapses() {
    let u = SpecType::Union {
        variants: vec![atom("ok")],
    };
    assert_eq!(u.canonicalise(), atom("ok"));
}

// A union of every shape kind: canonical ordering, nested-union flattening, atom dedup.
#[test]
fn canonicalise_flattens_sorts_and_dedups_diverse_variants() {
    let u = SpecType::Union {
        variants: vec![
            builtin("integer"),
            atom("error"),
            SpecType::Union {
                variants: vec![atom("ok"), SpecType::Integer { value: 0 }],
            },
            atom("ok"),
            SpecType::Tuple { arity: 2 },
            SpecType::Map,
            SpecType::List {
                element: Box::new(builtin("term")),
            },
            SpecType::Record { name: "cfg".into() },
            SpecType::TypeCall {
                module: Some("ra".into()),
                name: "index".into(),
                arity: 0,
            },
            SpecType::Var {
                name: "State".into(),
            },
            SpecType::Fun,
        ],
    };
    let SpecType::Union { variants } = u.canonicalise() else {
        panic!("expected a union");
    };
    assert_eq!(variants[0], atom("error"));
    assert_eq!(variants[1], atom("ok"));
    assert_eq!(variants.iter().filter(|v| **v == atom("ok")).count(), 1);
}

#[test]
fn integer_literal_matches_integer_builtins_only() {
    let five = SpecType::Integer { value: 5 };
    assert!(five.matches(&builtin("integer")));
    assert!(five.matches(&builtin("non_neg_integer")));
    assert!(five.matches(&builtin("pos_integer")));
    assert!(!five.matches(&builtin("atom")));
    assert!(builtin("non_neg_integer").matches(&five));
}

#[test]
fn tagged_and_plain_tuples_match_on_arity() {
    let tagged = SpecType::TaggedTuple {
        tag: "ok".into(),
        arity: 2,
    };
    let plain = SpecType::Tuple { arity: 2 };
    assert!(tagged.matches(&plain));
    assert!(plain.matches(&tagged));
    assert!(!tagged.matches(&SpecType::Tuple { arity: 3 }));
    assert!(!tagged.matches(&SpecType::TaggedTuple {
        tag: "error".into(),
        arity: 2,
    }));
}

#[test]
fn type_calls_match_on_name_and_arity_ignoring_module() {
    let a = SpecType::TypeCall {
        module: Some("ra".into()),
        name: "index".into(),
        arity: 0,
    };
    let b = SpecType::TypeCall {
        module: None,
        name: "index".into(),
        arity: 0,
    };
    assert!(a.matches(&b));
    assert!(!a.matches(&SpecType::TypeCall {
        module: Some("ra".into()),
        name: "index".into(),
        arity: 1,
    }));
}

#[test]
fn unknown_and_var_are_wildcards() {
    assert!(SpecType::Unknown.matches(&atom("ok")));
    assert!(atom("ok").matches(&SpecType::Unknown));
    let var = SpecType::Var { name: "X".into() };
    assert!(var.matches(&builtin("integer")));
    assert!(builtin("integer").matches(&var));
}

#[test]
fn collection_shapes_match_their_own_kind_only() {
    assert!(
        SpecType::List {
            element: Box::new(atom("ok")),
        }
        .matches(&SpecType::List {
            element: Box::new(builtin("term")),
        })
    );
    assert!(SpecType::Map.matches(&SpecType::Map));
    assert!(SpecType::Fun.matches(&SpecType::Fun));
    assert!(
        SpecType::Record { name: "cfg".into() }.matches(&SpecType::Record { name: "cfg".into() })
    );
    assert!(
        !SpecType::Record { name: "cfg".into() }.matches(&SpecType::Record {
            name: "state".into(),
        })
    );
    assert!(!SpecType::Map.matches(&SpecType::Fun));
}

// ra's apply/3 returns a union: it matches when any variant does.
#[test]
fn union_matches_when_any_variant_matches() {
    let u = SpecType::Union {
        variants: vec![atom("ok"), builtin("integer")],
    };
    assert!(u.matches(&builtin("integer")));
    assert!(builtin("integer").matches(&u));
    assert!(!u.matches(&atom("error")));
}

#[test]
fn atom_and_builtin_match_on_name() {
    assert!(atom("ok").matches(&builtin("ok")));
    assert!(builtin("ok").matches(&atom("ok")));
    assert!(!atom("ok").matches(&atom("error")));
}
