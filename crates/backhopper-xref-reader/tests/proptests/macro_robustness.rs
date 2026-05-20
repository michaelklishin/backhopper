// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_xref_reader::parse_define;

proptest! {
    /// `parse_define` must accept the trimmed inner form `NAME, body`
    /// and the paren-wrapped form `(NAME, body)` interchangeably.
    #[test]
    fn parse_define_accepts_both_forms(
        name in "[A-Z][A-Z_]{0,8}",
        body in "[a-z][a-z0-9_]{0,8}",
    ) {
        let inner = format!("{name}, {body}");
        let outer = format!("({name}, {body})");
        let r1 = parse_define(&inner).expect("inner form");
        let r2 = parse_define(&outer).expect("outer form");
        prop_assert_eq!(r1.0.name.clone(), r2.0.name);
        prop_assert_eq!(r1.0.arity, r2.0.arity);
        prop_assert_eq!(r1.1, r2.1);
        prop_assert_eq!(r1.0.name, name);
    }

    /// `parse_define` must record parameterized macro arity from the
    /// parameter list, regardless of the parameter names.
    #[test]
    fn parameterized_define_arity_matches_param_count(
        name in "[A-Z][A-Z_]{0,6}",
        params in proptest::collection::vec("[A-Z][a-zA-Z0-9_]{0,4}", 1..6),
        body in "[a-z][a-z0-9_]{0,8}\\([a-zA-Z, ]{0,16}\\)",
    ) {
        let param_str = params.join(", ");
        let raw = format!("({name}({param_str}), {body})");
        let (key, _) = parse_define(&raw).expect("parse");
        prop_assert_eq!(key.name, name);
        prop_assert_eq!(key.arity.unwrap() as usize, params.len());
    }

    /// `parse_define` must never panic on arbitrary ASCII bodies.
    #[test]
    fn parse_define_is_panic_free(text in "[\\x20-\\x7e\\n]{0,128}") {
        let _ = parse_define(&text);
    }
}
