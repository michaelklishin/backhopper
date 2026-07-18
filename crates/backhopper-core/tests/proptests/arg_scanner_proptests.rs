// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Properties of the unified argument scanner: a generated call with a
//! known arity always counts exactly, and truncating the closing paren
//! away always yields `Unterminated`, never a wrong `Exact`.

use proptest::prelude::*;

use backhopper_core::compat::call_sites::{ScannedArgs, scan_top_level_args};

// One argument term with commas only inside nested delimiters, strings, or char literals.
fn arg_term() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("X".to_owned()),
        Just("atom_value".to_owned()),
        Just("42".to_owned()),
        Just("{a, b, c}".to_owned()),
        Just("[1, 2, 3]".to_owned()),
        Just("{nested, [deep, {pair, 1}], end_}".to_owned()),
        Just("<<1,2,3>>".to_owned()),
        Just("<<\"bin, text\">>".to_owned()),
        Just("\"string, with, commas\"".to_owned()),
        Just("'quoted, atom'".to_owned()),
        Just("$,".to_owned()),
        Just("$\\\\".to_owned()),
        Just("inner:call(A, B)".to_owned()),
        Just("fun other:thing/2".to_owned()),
    ]
}

prop_compose! {
    fn call_args()(args in prop::collection::vec(arg_term(), 0..6)) -> Vec<String> {
        args
    }
}

proptest! {
    #[test]
    fn terminated_count_matches_the_generator(args in call_args()) {
        let line = format!("{})", args.join(", "));
        match scan_top_level_args(&line) {
            ScannedArgs::Terminated { args: scanned, .. } => {
                prop_assert_eq!(scanned.len(), args.len());
            }
            ScannedArgs::Unterminated { .. } => {
                prop_assert!(false, "generated call must terminate: {}", line);
            }
        }
    }

    #[test]
    fn truncation_never_fabricates_an_exact_arity(args in call_args()) {
        // Drop the closing paren: the scanner cannot determine the arity and must say so.
        let line = args.join(", ");
        let unterminated = matches!(
            scan_top_level_args(&line),
            ScannedArgs::Unterminated { .. }
        );
        prop_assert!(unterminated, "truncated call counted an arity: {}", line);
    }

    #[test]
    fn consumed_points_past_the_closing_paren(args in call_args()) {
        let body = format!("{})", args.join(", "));
        let line = format!("{body}, trailing:noise(1)");
        match scan_top_level_args(&line) {
            ScannedArgs::Terminated { consumed, .. } => {
                prop_assert_eq!(consumed, body.len());
            }
            ScannedArgs::Unterminated { .. } => {
                prop_assert!(false, "generated call must terminate: {}", line);
            }
        }
    }
}
