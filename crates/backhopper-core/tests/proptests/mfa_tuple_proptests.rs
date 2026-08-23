// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::compat::call_sites::extract_mfa_tuples_with_macros;
use backhopper_core::erlang_macros::MacroTable;

/// A generated atom in module or function position. Reserved words are
/// excluded: they are not legal bare atoms.
fn arb_atom(max_len: usize) -> impl Strategy<Value = String> {
    let pattern = format!("[a-z][a-z0-9_]{{0,{max_len}}}");
    proptest::string::string_regex(&pattern)
        .unwrap()
        .prop_filter("reserved words are not bare atoms", |s| {
            !RESERVED_WORDS.contains(&s.as_str())
        })
}

const RESERVED_WORDS: &[&str] = &[
    "after", "and", "andalso", "band", "begin", "bnot", "bor", "bsl", "bsr", "bxor", "case",
    "catch", "cond", "div", "end", "fun", "if", "let", "not", "of", "or", "orelse", "receive",
    "rem", "try", "when", "xor",
];

fn mfas(source: &str) -> Vec<String> {
    extract_mfa_tuples_with_macros(source, &MacroTable::new())
        .iter()
        .map(|mfa| mfa.to_string())
        .collect()
}

/// One level of nesting to wrap a target tuple in, each carrying an
/// unrelated atom sibling so the wrapper is not itself shape-tested.
#[derive(Debug, Clone)]
enum Wrap {
    Tuple(String),
    List(String),
}

fn arb_wrap() -> impl Strategy<Value = Wrap> {
    prop_oneof![
        arb_atom(6).prop_map(Wrap::Tuple),
        arb_atom(6).prop_map(Wrap::List),
    ]
}

proptest! {
    /// The extractor must never panic on arbitrary printable input.
    #[test]
    fn extract_mfa_tuples_is_panic_free(text in "[\\x20-\\x7e]{0,256}") {
        let _ = mfas(&text);
    }

    /// A matched tuple's arity always equals the literal length of the
    /// list it was read from.
    #[test]
    fn a_matched_tuples_arity_equals_the_literal_list_length(
        module in arb_atom(7),
        function in arb_atom(7),
        list_arity in 0u8..8,
    ) {
        let list = (0..list_arity)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("{{{module}, {function}, [{list}]}}");
        let out = mfas(&source);
        prop_assert_eq!(out, vec![format!("{module}:{function}/{list_arity}")]);
    }

    /// A variable in any of the three tuple positions must never yield
    /// a reference.
    #[test]
    fn a_variable_in_any_position_never_yields(
        position in 0usize..3,
        module in arb_atom(6),
        function in arb_atom(6),
    ) {
        let (m, f, a) = match position {
            0 => ("Var".to_owned(), function.clone(), "[]".to_owned()),
            1 => (module.clone(), "Var".to_owned(), "[]".to_owned()),
            _ => (module.clone(), function.clone(), "Var".to_owned()),
        };
        let out = mfas(&format!("{{{m}, {f}, {a}}}"));
        prop_assert!(out.is_empty(), "unexpected: {out:?}");
    }

    /// A well-formed MFA tuple planted at an arbitrary tuple-and-list
    /// nesting depth is always found: the descent must reach it
    /// regardless of how many groups surround it.
    #[test]
    fn a_planted_tuple_is_found_at_any_nesting_depth(
        module in arb_atom(6),
        function in arb_atom(6),
        wraps in proptest::collection::vec(arb_wrap(), 0..4),
    ) {
        let mut text = format!("{{{module}, {function}, []}}");
        for wrap in &wraps {
            text = match wrap {
                Wrap::Tuple(sibling) => format!("{{{sibling}, {text}}}"),
                Wrap::List(sibling) => format!("[{text}, {sibling}]"),
            };
        }
        let expected = format!("{module}:{function}/0");
        let out = mfas(&text);
        prop_assert!(out.contains(&expected), "want {expected} in {out:?} from {text}");
    }
}
