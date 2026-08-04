// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::spec_ast::SpecType;
use backhopper_core::model::spec_parser::{parse, parse_signature_return};
use proptest::prelude::*;

fn arb_spec_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_(){}\\[\\] ,:|@<>='#?\\-/.\\\\\"]{0,160}".prop_map(|s| s)
}

// generators for grammar the parser models fully: parse must yield a
// real node, never Unknown
fn arb_modelled_leaf() -> impl Strategy<Value = String> {
    prop_oneof![
        (0i64..1000, 1000i64..100_000).prop_map(|(a, b)| format!("{a}..{b}")),
        Just("$a..$z".to_owned()),
        Just("16#FF".to_owned()),
        Just("1_000".to_owned()),
        Just("<<>>".to_owned()),
        Just("<<_:8>>".to_owned()),
        Just("<<_:_*8>>".to_owned()),
        Just("dynamic()".to_owned()),
        Just("nonempty_binary()".to_owned()),
        Just("nonempty_bitstring()".to_owned()),
        Just("nonempty_improper_list()".to_owned()),
        Just("nonempty_maybe_improper_list()".to_owned()),
    ]
}

fn arb_modelled_type() -> impl Strategy<Value = String> {
    arb_modelled_leaf().prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..4).prop_map(|vs| vs.join(" | ")),
            prop::collection::vec(inner.clone(), 1..4)
                .prop_map(|vs| format!("{{frame, {}}}", vs.join(", "))),
            inner.clone().prop_map(|t| format!("Annotated :: {t}")),
            inner.prop_map(|t| format!("[{t}]")),
        ]
    })
}

fn contains_unknown(t: &SpecType) -> bool {
    match t {
        SpecType::Unknown => true,
        SpecType::Union { variants } => variants.iter().any(contains_unknown),
        SpecType::List { element } => contains_unknown(element),
        _ => false,
    }
}

proptest! {
    #[test]
    fn parse_never_panics(text in arb_spec_text()) {
        let _ = parse(&text);
    }

    #[test]
    fn parse_signature_return_never_panics(text in arb_spec_text()) {
        let _ = parse_signature_return(&text);
    }

    #[test]
    fn modelled_grammar_parses_without_unknown(text in arb_modelled_type()) {
        let t = parse(&text);
        prop_assert!(!contains_unknown(&t), "{text} parsed to {t:?}");
    }

    #[test]
    fn canonicalise_is_idempotent(text in arb_spec_text()) {
        let a = parse(&text);
        let b = a.clone().canonicalise();
        prop_assert_eq!(a, b);
    }
}
