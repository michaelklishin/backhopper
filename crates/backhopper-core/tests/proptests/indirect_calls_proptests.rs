// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Properties of `extract_indirect_calls`: total on arbitrary input,
//! reformat-invariant on well-formed forms, and accounting-complete at
//! the extraction level.

use backhopper_core::compat::indirect_calls::extract_indirect_calls;
use proptest::prelude::*;

fn identity_line_map(src: &str) -> Vec<u32> {
    (1..=src.lines().count() as u32).collect()
}

fn atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,10}"
}

/// One well-formed occurrence with a literal module and function. The
/// bool picks the arity source: an integer meck expectation or an rpc
/// call with a literal list.
fn occurrence() -> impl Strategy<Value = String> {
    (atom(), atom(), 0usize..5, any::<bool>()).prop_map(|(m, f, arity, as_meck)| {
        if as_meck {
            format!("meck:expect({m}, {f}, {arity}, ok)")
        } else {
            let args = vec!["x"; arity].join(", ");
            format!("rpc:call(Node, {m}, {f}, [{args}])")
        }
    })
}

proptest! {
    #[test]
    fn never_panics_on_arbitrary_text(src in ".{0,300}") {
        let line_map = identity_line_map(&src);
        let _ = extract_indirect_calls(&src, &line_map);
    }

    #[test]
    fn never_panics_on_erlangish_text(src in "[a-z_:,\\(\\)\\[\\]\\{\\}'\"\\|\\$ \n]{0,300}") {
        let line_map = identity_line_map(&src);
        let _ = extract_indirect_calls(&src, &line_map);
    }

    // Wrapping a form's argument list across lines never changes the
    // extraction, as long as the lines stay file-adjacent.
    #[test]
    fn a_line_break_after_a_top_level_comma_changes_nothing(
        occ in occurrence(),
        break_at in 0usize..4,
    ) {
        let flat = format!("t() -> {occ}.\n");
        let out_flat = extract_indirect_calls(&flat, &identity_line_map(&flat));

        // Break after the `break_at`-th top-level comma of the form,
        // when it has one.
        let mut commas = 0usize;
        let mut depth = 0i32;
        let mut split = None;
        for (i, c) in occ.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 1 => {
                    if commas == break_at {
                        split = Some(i + 1);
                        break;
                    }
                    commas += 1;
                }
                _ => {}
            }
        }
        let Some(split) = split else {
            return Ok(());
        };
        let wrapped = format!("t() -> {}\n{}.\n", &occ[..split], &occ[split..]);
        let out_wrapped = extract_indirect_calls(&wrapped, &identity_line_map(&wrapped));

        prop_assert_eq!(
            out_flat.sites.iter().map(|s| (s.mfa.clone(), s.via)).collect::<Vec<_>>(),
            out_wrapped.sites.iter().map(|s| (s.mfa.clone(), s.via)).collect::<Vec<_>>()
        );
        prop_assert_eq!(out_flat.withheld_dynamic, out_wrapped.withheld_dynamic);
    }

    // Extraction-level accounting: every generated occurrence has a
    // literal module and function, so each one lands in `sites` or in
    // `withheld_dynamic`, never nowhere.
    #[test]
    fn every_literal_occurrence_is_accounted_for(occs in prop::collection::vec(occurrence(), 1..6)) {
        let body: Vec<String> = occs.iter().map(|o| format!("    {o},")).collect();
        let src = format!("t(Node) ->\n{}\n    ok.\n", body.join("\n"));
        let out = extract_indirect_calls(&src, &identity_line_map(&src));
        prop_assert_eq!(out.sites.len() + out.withheld_dynamic, occs.len());
    }
}
