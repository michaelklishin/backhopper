// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::snapshot::spec_normalize::normalize_signature;

#[test]
fn normalizer_collapses_whitespace_runs() {
    assert_eq!(
        normalize_signature("apply(  X  ,  Y )    ->   ok"),
        "apply( X , Y ) -> ok"
    );
}

#[test]
fn normalizer_is_idempotent_for_short_inputs() {
    let s = "apply(X) -> ok";
    let once = normalize_signature(s);
    let twice = normalize_signature(&once);
    assert_eq!(once, twice);
}

#[test]
fn normalizer_does_not_break_quoted_strings() {
    let s = r#"apply(X) -> "a    b" | binary"#;
    let n = normalize_signature(s);
    assert!(n.starts_with(r#"apply(X) -> "a    b""#) || n.contains(r#""a    b""#));
}

#[test]
fn normalizer_wraps_long_top_level_alternatives() {
    let s = "process_command(ServerId, Command) -> {ok, term(), ra_server_id()} | {timeout, ra_server_id()} | {error, term()} | {error_b, term()} | {error_c, term()}";
    let n = normalize_signature(s);
    assert!(
        n.contains('\n'),
        "expected wrapping for long alternatives, got {n:?}"
    );
}

#[test]
fn char_literal_quote_does_not_open_a_string() {
    // whitespace after `$"` must still collapse: the quote is a char
    // literal, not a string opener
    assert_eq!(
        normalize_signature("quote_char() ->   $\"   |   eof"),
        "quote_char() -> $\" | eof"
    );
}

#[test]
fn char_literal_space_is_preserved() {
    assert_eq!(normalize_signature("space() ->   $ "), "space() -> $ ");
}

#[test]
fn char_literal_comma_and_parens_pass_through() {
    assert_eq!(
        normalize_signature("{sep,  $,,  open,  $(,  close,  $)}"),
        "{sep, $,, open, $(, close, $)}"
    );
}

#[test]
fn char_literal_percent_does_not_start_a_comment() {
    assert_eq!(
        normalize_signature("percent() -> $% | eof"),
        "percent() -> $% | eof"
    );
}
