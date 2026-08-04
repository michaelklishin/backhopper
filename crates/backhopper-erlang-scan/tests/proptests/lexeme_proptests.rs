// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-MIT and LICENSE-APACHE for details.

use proptest::prelude::*;

use backhopper_erlang_scan::{
    dot_terminates, number_span, quoted_atom_span, skip_char_literal_span, string_span,
};

fn valid_number() -> impl Strategy<Value = String> {
    prop_oneof![
        "[1-9][0-9]{0,6}",
        "[1-9][0-9]{0,2}(_[0-9]{1,3}){1,3}",
        "2#[01]{1,6}",
        "8#[0-7]{1,6}",
        "16#[0-9a-fA-F]{1,6}",
        "36#[0-9a-zA-Z]{1,6}",
        "[0-9]{1,4}\\.[0-9]{1,4}",
        "[0-9]{1,4}\\.[0-9]{1,4}[eE][+-]?[0-9]{1,3}",
        "16#[0-9a-f]{1,4}\\.[0-9a-f]{1,4}",
        "16#[0-9a-f]{1,4}\\.[0-9a-f]{1,4}#[eE][+-]?[0-9]{1,3}",
    ]
}

fn char_literal() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\$[a-zA-Z0-9]",
        Just("$\\n".to_string()),
        Just("$\\'".to_string()),
        Just("$\\^G".to_string()),
        Just("$\\xFF".to_string()),
        Just("$\\x{1F600}".to_string()),
        Just("$\\377".to_string()),
    ]
}

proptest! {
    #[test]
    fn no_primitive_panics_on_arbitrary_bytes(bytes: Vec<u8>, at in 0usize..64) {
        if at < bytes.len() {
            let _ = string_span(&bytes, at);
            let _ = quoted_atom_span(&bytes, at);
            let _ = number_span(&bytes, at);
            let _ = dot_terminates(&bytes, at);
            if bytes[at] == b'$' {
                let _ = skip_char_literal_span(&bytes, at);
            }
        }
    }

    #[test]
    fn spans_never_reach_past_the_end_of_the_input(bytes: Vec<u8>, at in 0usize..64) {
        if at < bytes.len() {
            for span in [
                string_span(&bytes, at),
                quoted_atom_span(&bytes, at),
                number_span(&bytes, at),
            ]
            .into_iter()
            .flatten()
            {
                prop_assert!(at + span <= bytes.len());
            }
        }
    }

    #[test]
    fn number_span_covers_exactly_a_generated_valid_literal(lit in valid_number(), tail in "[ ,)%]{0,3}") {
        let src = format!("{lit}{tail}");
        prop_assert_eq!(number_span(src.as_bytes(), 0), Some(lit.len()));
    }

    #[test]
    fn char_literal_leaves_the_cursor_exactly_past_the_literal(lit in char_literal(), rest in "[ a-z(){},.\"']{0,8}") {
        let src = format!("{lit}{rest}");
        prop_assert_eq!(skip_char_literal_span(src.as_bytes(), 0), lit.len());
    }
}
