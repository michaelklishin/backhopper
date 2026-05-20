// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::snapshot::spec_normalize::normalize_signature;

#[test]
fn normalizer_collapses_whitespace_runs() {
    assert_eq!(
        normalize_signature("foo(  X  ,  Y )    ->   ok"),
        "foo( X , Y ) -> ok"
    );
}

#[test]
fn normalizer_is_idempotent_for_short_inputs() {
    let s = "foo(X) -> ok";
    let once = normalize_signature(s);
    let twice = normalize_signature(&once);
    assert_eq!(once, twice);
}

#[test]
fn normalizer_does_not_break_quoted_strings() {
    let s = r#"foo(X) -> "a    b" | bar"#;
    let n = normalize_signature(s);
    assert!(n.starts_with(r#"foo(X) -> "a    b""#) || n.contains(r#""a    b""#));
}

#[test]
fn normalizer_wraps_long_top_level_alternatives() {
    let s = "process_command(ServerId, Command) -> {ok, term(), ra_server_id()} | {timeout, ra_server_id()} | {error, term()} | {error_b, term()} | {error_c, term()}";
    let n = normalize_signature(s);
    assert!(
        n.contains('\n'),
        "expected wrapping for long alternatives, got {:?}",
        n
    );
}