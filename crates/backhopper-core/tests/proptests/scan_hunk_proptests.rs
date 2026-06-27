// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The keystone safety property: for a hunk whose constructs each fit on
//! one line, `scan_hunk` produces the same references as scanning each
//! line on its own. This bounds the join's effect to multi-line
//! constructs.

use backhopper_core::compat::call_sites::{extract_into_with_macros, scan_hunk};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::model::symbol::{RefOrigin, SymbolRef};
use proptest::prelude::*;

// One indented, single-line construct of each kind the body extractor
// reads: a call, a record use, a macro use, and a remote fun ref.
// Indented so it is never read as a clause head; lowercase modules so a
// call is never dynamic dispatch.
fn arb_construct_line() -> impl Strategy<Value = String> {
    let ident = "[a-z][a-z0-9_]{0,5}";
    prop_oneof![
        (ident, ident, "[a-z0-9_]{0,3}").prop_map(|(m, f, a)| format!("    {m}:{f}({a})")),
        ident.prop_map(|r| format!("    #{r}{{}}")),
        "[A-Z][A-Z0-9_]{0,5}".prop_map(|m| format!("    ?{m}")),
        (ident, ident, "[0-9]").prop_map(|(m, f, a)| format!("    fun {m}:{f}/{a}")),
    ]
}

fn per_line(lines: &[(RefOrigin, String)]) -> Vec<SymbolRef> {
    let macros = MacroTable::new();
    let mut out = Vec::new();
    for (origin, text) in lines {
        let mut refs = Vec::new();
        extract_into_with_macros(text, &macros, &mut refs);
        out.extend(refs.into_iter().map(|r| r.with_origin(*origin)));
    }
    out
}

proptest! {
    #[test]
    fn scan_hunk_matches_per_line_for_single_line_body(
        lines in prop::collection::vec(
            (any::<bool>(), arb_construct_line()),
            0..8,
        )
    ) {
        let lines: Vec<(RefOrigin, String)> = lines
            .into_iter()
            .map(|(added, s)| (if added { RefOrigin::Added } else { RefOrigin::Context }, s))
            .collect();
        let borrowed: Vec<(RefOrigin, &str)> =
            lines.iter().map(|(o, s)| (*o, s.as_str())).collect();

        let mut got = scan_hunk(&borrowed, &MacroTable::new()).referenced;
        let mut expected = per_line(&lines);
        got.sort();
        expected.sort();
        prop_assert_eq!(got, expected);
    }
}
