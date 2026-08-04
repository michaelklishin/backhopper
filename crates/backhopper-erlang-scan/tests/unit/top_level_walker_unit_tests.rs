// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `for_each_top_level_byte` and `is_bare_atom`.

use backhopper_erlang_scan::{for_each_top_level_byte, is_bare_atom};

fn find(s: &str, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    for_each_top_level_byte(s, |i| bytes[i] == target)
}

#[test]
fn finds_a_top_level_comma() {
    assert_eq!(find("ra_server, log", b','), Some(9));
}

#[test]
fn skips_commas_inside_brackets() {
    assert_eq!(find("{ra, khepri}, tail", b','), Some(12));
    assert_eq!(find("[a, b]", b','), None);
    assert_eq!(find("f(A, B)", b','), None);
}

#[test]
fn skips_commas_inside_binaries() {
    assert_eq!(find("<<_:8, _:_*8>>, rest", b','), Some(14));
}

#[test]
fn char_literal_comma_is_inert() {
    assert_eq!(find("$,", b','), None);
    assert_eq!(find("$, , next", b','), Some(3));
}

#[test]
fn char_literal_paren_does_not_open_depth() {
    assert_eq!(find("$( , next", b','), Some(3));
    assert_eq!(find("$) , next", b','), Some(3));
}

#[test]
fn char_literal_quote_does_not_open_a_string() {
    assert_eq!(find("$\" , next", b','), Some(3));
}

#[test]
fn strings_and_quoted_atoms_are_opaque() {
    assert_eq!(find("\"a,b\", tail", b','), Some(5));
    assert_eq!(find("'a,b', tail", b','), Some(5));
}

#[test]
fn triple_quoted_strings_are_opaque() {
    let s = "\"\"\"\ntext, with (commas\n\"\"\", tail";
    let bytes = s.as_bytes();
    let found = find(s, b',').expect("comma after triple quote");
    assert_eq!(bytes[found], b',');
    assert!(s[found..].starts_with(", tail"));
}

#[test]
fn comments_are_opaque() {
    assert_eq!(find("a % note, with (paren\n, tail", b','), Some(22));
}

#[test]
fn visits_every_top_level_byte_without_a_match() {
    let mut seen = Vec::new();
    let s = "a(b), c";
    assert_eq!(
        for_each_top_level_byte(s, |i| {
            seen.push(s.as_bytes()[i]);
            false
        }),
        None
    );
    assert_eq!(seen, b"a, c".to_vec());
}

#[test]
fn bare_atoms_are_recognized() {
    assert!(is_bare_atom("ra_server"));
    assert!(is_bare_atom("rabbit_misc"));
    assert!(is_bare_atom("v1@node"));
}

#[test]
fn non_atoms_are_rejected() {
    assert!(!is_bare_atom(""));
    assert!(!is_bare_atom("Var"));
    assert!(!is_bare_atom("_ignored"));
    assert!(!is_bare_atom("'quoted'"));
    assert!(!is_bare_atom("ra server"));
    assert!(!is_bare_atom("1init"));
}
