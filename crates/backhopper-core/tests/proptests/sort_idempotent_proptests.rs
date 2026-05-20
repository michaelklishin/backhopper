// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::{FunArity, HrlFile, Module};
use backhopper_core::snapshot::sort::canonicalize;

fn arb_lower_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,8}".prop_map(|s| s)
}

fn arb_module() -> impl Strategy<Value = Module> {
    (
        arb_lower_atom(),
        prop::collection::vec((arb_lower_atom(), 0u8..=4), 0..6),
    )
        .prop_map(|(name, exports)| {
            let mut m = Module::new(ModuleName::new(name).unwrap());
            for (n, a) in exports {
                m.exports.push(FunArity {
                    name: FunctionName::new(n).unwrap(),
                    arity: Arity::new(a),
                });
            }
            m
        })
}

proptest! {
    #[test]
    fn canonicalize_is_idempotent(
        modules in prop::collection::vec(arb_module(), 0..6)
    ) {
        let mut a = modules.clone();
        let mut h: Vec<HrlFile> = Vec::new();
        canonicalize(&mut a, &mut h);
        let snapshot = a.clone();
        canonicalize(&mut a, &mut h);
        prop_assert_eq!(a, snapshot);
    }
}
