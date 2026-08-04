// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The shared lexeme primitives: string, quoted atom, char literal,
//! number, and dot-terminator spans.

use backhopper_erlang_scan::{
    dot_terminates, hash_inside_number, number_span, quoted_atom_span, sigil_span,
    skip_char_literal_span, string_span, triple_quoted_span,
};

fn char_span(src: &str) -> usize {
    skip_char_literal_span(src.as_bytes(), 0)
}

fn num_span(src: &str) -> Option<usize> {
    number_span(src.as_bytes(), 0)
}

#[test]
fn string_span_agrees_with_the_underlying_primitives() {
    let ordinary = r#""queue name" rest"#;
    assert_eq!(string_span(ordinary.as_bytes(), 0), Some(12));

    let triple = "\"\"\"\nprose with \" quotes.\n\"\"\" rest";
    assert_eq!(
        string_span(triple.as_bytes(), 0),
        triple_quoted_span(triple.as_bytes(), 0)
    );

    let sigil = r#"~s(ra log dir) rest"#;
    assert_eq!(
        string_span(sigil.as_bytes(), 0),
        sigil_span(sigil.as_bytes(), 0)
    );
}

#[test]
fn string_span_returns_none_off_an_opener() {
    assert_eq!(string_span(b"khepri", 0), None);
    assert_eq!(string_span(b"~ not a sigil", 0), None);
    assert_eq!(string_span(b"", 0), None);
}

#[test]
fn unterminated_ordinary_string_runs_to_end_of_input() {
    assert_eq!(string_span(b"\"no closer", 0), Some(10));
}

#[test]
fn string_span_honors_escaped_quotes() {
    assert_eq!(string_span(br#""a\"b" rest"#, 0), Some(6));
}

#[test]
fn quoted_atom_span_covers_escapes_and_newlines() {
    assert_eq!(quoted_atom_span(br"'a\'b' rest", 0), Some(6));
    assert_eq!(quoted_atom_span(b"'two\nlines' rest", 0), Some(11));
    assert_eq!(quoted_atom_span(b"'unterminated", 0), Some(13));
    assert_eq!(quoted_atom_span(b"atom", 0), None);
}

#[test]
fn char_literal_escape_spans() {
    assert_eq!(char_span("$\\x{1F600} rest"), 10);
    assert_eq!(char_span("$\\xFF rest"), 5);
    assert_eq!(char_span("$\\377 rest"), 5);
    assert_eq!(char_span("$\\7, rest"), 3);
    assert_eq!(char_span("$\\^G rest"), 4);
    assert_eq!(char_span("$\\' rest"), 3);
    assert_eq!(char_span("$\" rest"), 2);
    assert_eq!(char_span("$  rest"), 2);
    assert_eq!(char_span("$\\x{"), 4);
}

#[test]
fn number_span_integers_with_underscores() {
    assert_eq!(num_span("1_000_000, X"), Some(9));
    assert_eq!(num_span("42"), Some(2));
    assert_eq!(num_span("1_"), Some(1));
}

#[test]
fn number_span_base_boundaries() {
    assert_eq!(num_span("2#1010, rest"), Some(6));
    assert_eq!(num_span("16#ff, rest"), Some(5));
    assert_eq!(num_span("36#zz, rest"), Some(5));
    assert_eq!(num_span("37#zz, rest"), Some(2));
    assert_eq!(num_span("16#FF, rest"), Some(5));
    assert_eq!(num_span("2#1012"), Some(5));
}

#[test]
fn number_span_decimal_floats() {
    assert_eq!(num_span("3.14, X"), Some(4));
    assert_eq!(num_span("1.0e10 rest"), Some(6));
    assert_eq!(num_span("1.0e-5 rest"), Some(6));
    assert_eq!(num_span("2.5E+3 rest"), Some(6));
    assert_eq!(num_span("1.0e rest"), Some(3));
}

#[test]
fn number_span_based_floats() {
    assert_eq!(num_span("16#fe.fe rest"), Some(8));
    assert_eq!(num_span("16#fe.fe#e16 rest"), Some(12));
    assert_eq!(num_span("16#fe.fe#E-2 rest"), Some(12));
    assert_eq!(num_span("16#fe.fe#x rest"), Some(8));
}

#[test]
fn number_span_malformed_tails_end_at_the_last_valid_position() {
    assert_eq!(num_span("10abc"), Some(2));
    assert_eq!(num_span("16#"), Some(2));
    assert_eq!(num_span("1_"), Some(1));
    assert_eq!(num_span("1..2"), Some(1));
}

#[test]
fn number_span_only_fires_at_a_digit() {
    assert_eq!(number_span(b"x16#ff", 0), None);
    assert_eq!(number_span(b"#ff", 0), None);
}

#[test]
fn dot_terminates_cases() {
    assert!(dot_terminates(b". next", 0));
    assert!(dot_terminates(b".\nnext", 0));
    assert!(dot_terminates(b".% comment", 0));
    assert!(dot_terminates(b".", 0));
    assert!(!dot_terminates(b".5", 0));
    assert!(!dot_terminates(b".field", 0));
    assert!(!dot_terminates(b"x next", 0));
    assert!(!dot_terminates(b"", 0));
}

#[test]
fn hash_inside_number_suppresses_based_literal_hashes() {
    let src = b"X = 16#ff,";
    let at = src.iter().position(|&b| b == b'#').unwrap();
    assert!(hash_inside_number(src, at));

    let float = b"Y = 16#fe.fe#e16,";
    for (i, &b) in float.iter().enumerate() {
        if b == b'#' {
            assert!(hash_inside_number(float, i));
        }
    }
}

#[test]
fn hash_inside_number_keeps_record_references() {
    for src in [
        b"State2#state{count = 1}".as_slice(),
        b"X2 #state{}",
        b"#state{}",
    ] {
        let at = src.iter().position(|&b| b == b'#').unwrap();
        assert!(!hash_inside_number(src, at));
    }
    let two = b"2#1010, #state{}";
    let second = two.iter().rposition(|&b| b == b'#').unwrap();
    assert!(hash_inside_number(two, 1));
    assert!(!hash_inside_number(two, second));
}
