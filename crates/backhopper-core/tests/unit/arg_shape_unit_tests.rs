// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::arg_shape::{ArgShape, satisfies_any};
use backhopper_core::model::names::RecordName;

#[test]
fn unknown_actual_matches_any_pattern() {
    let pattern = ArgShape::Atom { name: "x".into() };
    assert!(ArgShape::Unknown.satisfies(&pattern));
}

#[test]
fn unknown_pattern_matches_any_actual() {
    let actual = ArgShape::Atom { name: "x".into() };
    assert!(actual.satisfies(&ArgShape::Unknown));
}

#[test]
fn variable_pattern_matches_any_actual() {
    let actual = ArgShape::Atom { name: "x".into() };
    assert!(actual.satisfies(&ArgShape::Variable));
}

#[test]
fn variable_actual_matches_any_pattern() {
    let pattern = ArgShape::Atom { name: "x".into() };
    assert!(ArgShape::Variable.satisfies(&pattern));
}

#[test]
fn matching_atom_names_satisfy() {
    let a = ArgShape::Atom { name: "ok".into() };
    let b = ArgShape::Atom { name: "ok".into() };
    assert!(a.satisfies(&b));
}

#[test]
fn differing_atom_names_do_not_satisfy() {
    let a = ArgShape::Atom { name: "ok".into() };
    let b = ArgShape::Atom {
        name: "error".into(),
    };
    assert!(!a.satisfies(&b));
}

#[test]
fn equal_tuple_sizes_satisfy() {
    assert!(ArgShape::Tuple { size: 2 }.satisfies(&ArgShape::Tuple { size: 2 }));
}

#[test]
fn differing_tuple_sizes_do_not_satisfy() {
    assert!(!ArgShape::Tuple { size: 2 }.satisfies(&ArgShape::Tuple { size: 3 }));
}

#[test]
fn equal_record_names_satisfy() {
    let user = RecordName::new("user").unwrap();
    let a = ArgShape::Record { name: user.clone() };
    let b = ArgShape::Record { name: user };
    assert!(a.satisfies(&b));
}

#[test]
fn differing_record_names_do_not_satisfy() {
    let a = ArgShape::Record {
        name: RecordName::new("user").unwrap(),
    };
    let b = ArgShape::Record {
        name: RecordName::new("admin").unwrap(),
    };
    assert!(!a.satisfies(&b));
}

#[test]
fn different_concrete_kinds_do_not_satisfy() {
    assert!(!ArgShape::Integer.satisfies(&ArgShape::Float));
    assert!(!ArgShape::List.satisfies(&ArgShape::Binary));
    assert!(!ArgShape::String.satisfies(&ArgShape::Fun));
}

#[test]
fn empty_clauses_always_satisfies() {
    let args = vec![ArgShape::Atom { name: "x".into() }];
    assert!(satisfies_any(&args, &[]));
}

#[test]
fn clause_with_different_arity_does_not_match() {
    let args = vec![ArgShape::Atom { name: "x".into() }];
    let clauses = vec![vec![ArgShape::Variable, ArgShape::Variable]];
    assert!(!satisfies_any(&args, &clauses));
}

#[test]
fn first_clause_matches_short_circuits() {
    let args = vec![ArgShape::Atom {
        name: "start".into(),
    }];
    let clauses = vec![
        vec![ArgShape::Atom {
            name: "start".into(),
        }],
        vec![ArgShape::Atom {
            name: "stop".into(),
        }],
    ];
    assert!(satisfies_any(&args, &clauses));
}

#[test]
fn no_clause_matches_returns_false() {
    let args = vec![ArgShape::Atom {
        name: "restart".into(),
    }];
    let clauses = vec![
        vec![ArgShape::Atom {
            name: "start".into(),
        }],
        vec![ArgShape::Atom {
            name: "stop".into(),
        }],
    ];
    assert!(!satisfies_any(&args, &clauses));
}