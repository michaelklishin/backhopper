// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::model::snapshot::{FunArity, TypeArity};

fn arb_lower_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,12}".prop_map(|s| s)
}

fn arb_arity() -> impl Strategy<Value = u8> {
    0u8..=20
}

proptest! {
    #[test]
    fn fun_arity_from_str_round_trip(name in arb_lower_atom(), arity in arb_arity()) {
        let s = format!("{name}/{arity}");
        let parsed: FunArity = s.parse().unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn type_arity_from_str_round_trip(name in arb_lower_atom(), arity in arb_arity()) {
        let s = format!("{name}/{arity}");
        let parsed: TypeArity = s.parse().unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }
}
