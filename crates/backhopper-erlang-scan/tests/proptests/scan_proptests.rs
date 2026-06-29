// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_erlang_scan::{
    ScannedArgs, count_top_level_commas, count_top_level_items, scan_arity, scan_top_level_args,
    split_top_level_args, split_top_level_commas, take_balanced_parens,
};

// Each term is one top-level item that carries an internal comma the
// scanner must not treat as a separator: a tuple, list, string, quoted
// atom, and bitstring, one per bracket and quote family, plus a plain
// atom.
const TERMS: &[&str] = &[
    "ra_log",
    "{ok, State}",
    "[a, b, c]",
    "\"x, y\"",
    "'quoted, atom'",
    "<<_:8, _:_*8>>",
];

fn term_list() -> impl Strategy<Value = Vec<&'static str>> {
    prop::collection::vec(prop::sample::select(TERMS), 1..8)
}

proptest! {
    // Gold standard: when the expected items are known by construction,
    // the splitter returns exactly them and the counter agrees. Internal
    // commas in any family never leak to the top level.
    #[test]
    fn split_recovers_generated_terms(terms in term_list()) {
        let s = terms.join(", ");
        prop_assert_eq!(split_top_level_commas(&s), terms.clone());
        prop_assert_eq!(count_top_level_commas(&s), terms.len() - 1);
    }

    // Splitter and counter share one decision: every piece a split
    // produces has no top-level comma of its own.
    #[test]
    fn each_split_piece_has_no_top_level_comma(s in "[\\PC]{0,256}") {
        for piece in split_top_level_commas(&s) {
            prop_assert_eq!(count_top_level_commas(piece), 0);
        }
    }

    // The top-level count can never exceed the raw comma count.
    #[test]
    fn top_level_count_is_bounded_by_raw_commas(s in "[\\PC]{0,256}") {
        let raw = s.matches(',').count();
        prop_assert!(count_top_level_commas(&s) <= raw);
    }

    // A `(args)` built from known terms scans as Terminated, recovers the
    // arity, and reports `consumed` just past the closing paren.
    #[test]
    fn scan_top_level_args_recovers_arity_and_consumed(terms in term_list()) {
        let s = format!("{})", terms.join(", "));
        match scan_top_level_args(&s) {
            ScannedArgs::Terminated { args, consumed } => {
                prop_assert_eq!(args.len(), terms.len());
                prop_assert_eq!(consumed, s.len());
                prop_assert_eq!(s.as_bytes()[consumed - 1], b')');
            }
            ScannedArgs::Unterminated { .. } => prop_assert!(false, "expected Terminated"),
        }
    }

    // `(inner)rest` with a balanced, quote-balanced inner round-trips.
    #[test]
    fn take_balanced_parens_round_trips(terms in term_list(), rest in " -> [a-z]{1,6}") {
        let inner = terms.join(", ");
        let s = format!("({inner}){rest}");
        prop_assert_eq!(
            take_balanced_parens(&s),
            Some((inner.as_str(), rest.as_str()))
        );
    }

    // No entry point panics on arbitrary input, including multibyte text
    // that could land a byte index inside a UTF-8 char.
    #[test]
    fn entry_points_never_panic(s in "\\PC{0,256}") {
        let _ = split_top_level_commas(&s);
        let _ = count_top_level_commas(&s);
        let _ = scan_top_level_args(&s);
        let _ = scan_arity(&s);
        let _ = split_top_level_args(&s);
        let _ = count_top_level_items(&s, '{', '}');
        let _ = take_balanced_parens(&s);
    }
}
