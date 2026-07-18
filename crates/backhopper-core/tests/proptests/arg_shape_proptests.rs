// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::compat::arg_shape::{ArgShape, satisfies_any};
use backhopper_core::model::names::RecordName;

fn arb_atom_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,4}".prop_map(|s| s)
}

fn arb_record_name() -> impl Strategy<Value = RecordName> {
    arb_atom_name().prop_map(|n| RecordName::new(&n).unwrap())
}

fn arb_shape() -> impl Strategy<Value = ArgShape> {
    prop_oneof![
        Just(ArgShape::Variable),
        arb_atom_name().prop_map(|name| ArgShape::Atom { name }),
        Just(ArgShape::Integer),
        Just(ArgShape::Float),
        Just(ArgShape::Binary),
        Just(ArgShape::List),
        (1usize..=8).prop_map(|size| ArgShape::Tuple { size }),
        arb_record_name().prop_map(|name| ArgShape::Record { name }),
        Just(ArgShape::String),
        Just(ArgShape::Fun),
        Just(ArgShape::Unknown),
    ]
}

proptest! {
    /// `Unknown` on the actual side never produces a definite mismatch.
    #[test]
    fn unknown_actual_always_satisfies(pattern in arb_shape()) {
        prop_assert!(ArgShape::Unknown.satisfies(&pattern));
    }

    /// `Unknown` as a clause-head pattern never excludes any actual.
    #[test]
    fn unknown_pattern_always_satisfies(actual in arb_shape()) {
        prop_assert!(actual.satisfies(&ArgShape::Unknown));
    }

    /// A clause-head variable (binds anything) accepts any actual.
    #[test]
    fn variable_pattern_always_satisfies(actual in arb_shape()) {
        prop_assert!(actual.satisfies(&ArgShape::Variable));
    }

    /// A call-site variable could hold any runtime value, so we can
    /// never prove a mismatch against any pattern.
    #[test]
    fn variable_actual_always_satisfies(pattern in arb_shape()) {
        prop_assert!(ArgShape::Variable.satisfies(&pattern));
    }

    /// `satisfies` is reflexive: every shape matches itself.
    #[test]
    fn satisfies_is_reflexive(shape in arb_shape()) {
        prop_assert!(shape.satisfies(&shape));
    }

    /// `satisfies_any` over an empty clause list is always true: no recorded pattern data means no mismatch.
    #[test]
    fn satisfies_any_empty_clauses_is_always_true(
        args in prop::collection::vec(arb_shape(), 0..6),
    ) {
        prop_assert!(satisfies_any(&args, &[]));
    }

    /// `satisfies_any` rejects clauses with mismatched arity even when
    /// every position would otherwise match.
    #[test]
    fn satisfies_any_rejects_arity_mismatch(
        args in prop::collection::vec(arb_shape(), 1..6),
        extra in arb_shape(),
    ) {
        let mut wider = args.clone();
        wider.push(extra);
        let clauses = vec![wider];
        prop_assert!(!satisfies_any(&args, &clauses));
    }

    /// If at least one clause is all-Variables and arities match, the
    /// disjunction is always satisfied: variables accept everything.
    #[test]
    fn satisfies_any_succeeds_when_one_clause_is_all_variables(
        args in prop::collection::vec(arb_shape(), 0..6),
    ) {
        let permissive = vec![ArgShape::Variable; args.len()];
        prop_assert!(satisfies_any(&args, &[permissive]));
    }
}
