// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::extract_into;

/// A generated atom in module or function position. Reserved words are
/// excluded: they are not legal bare atoms, and `apply(if, a, [])` is
/// not a Erlang expression any scanner has to read.
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
    let mut out = Vec::new();
    extract_into(source, &mut out);
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

proptest! {
    /// apply/3 with literal atoms and a literal list of `n` items
    /// must resolve to `module:function/n`.
    #[test]
    fn apply_3_with_atom_literals_always_resolves(
        module in arb_atom(7),
        function in arb_atom(7),
        list_arity in 0u8..8,
    ) {
        let list = (0..list_arity)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("apply({module}, {function}, [{list}])");
        let calls = mfas(&source);
        let expected = format!("{module}:{function}/{list_arity}");
        prop_assert!(calls.contains(&expected), "want {expected} in {calls:?}");
    }

    /// All spawn-family variants with literal arity-3 form should
    /// resolve into `M:F/length(A)`.
    #[test]
    fn spawn_family_with_atom_literals_resolves(
        family in prop::sample::select(vec![
            "spawn",
            "spawn_link",
            "spawn_monitor",
            "hibernate",
        ]),
        module in arb_atom(5),
        function in arb_atom(5),
        list_arity in 0u8..6,
    ) {
        let list = (0..list_arity)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("{family}({module}, {function}, [{list}])");
        let calls = mfas(&source);
        let expected = format!("{module}:{function}/{list_arity}");
        prop_assert!(calls.contains(&expected), "want {expected} in {calls:?}");
    }

    /// extract_into must never panic on arbitrary printable input.
    #[test]
    fn extract_into_is_panic_free(text in "[\\x20-\\x7e]{0,256}") {
        let _ = mfas(&text);
    }
}
