// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_qualified_calls`: statically-named `mod:fun(Args)` calls
//! with line and arity, excluding type references and dynamic dispatch.

use backhopper_core::compat::call_sites::extract_qualified_calls;

fn calls(src: &str) -> Vec<(String, String, u8, u32)> {
    extract_qualified_calls(src)
        .into_iter()
        .map(|c| {
            (
                c.mfa.module.to_string(),
                c.mfa.function.to_string(),
                c.mfa.arity.get(),
                c.line,
            )
        })
        .collect()
}

#[test]
fn a_qualified_call_is_captured_with_line_and_arity() {
    let got = calls("f(V, Q) -> rabbit_misc:queue_resource(V, Q).\n");
    assert_eq!(
        got,
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn the_line_is_the_source_line() {
    let got = calls("a() -> ok.\nb() -> ok.\nf() -> m:g(x).\n");
    assert_eq!(got, [("m".to_owned(), "g".to_owned(), 1, 3)]);
}

// A qualified type in a -spec is a type reference, not a call.
#[test]
fn a_qualified_type_in_a_spec_is_skipped() {
    let got = calls("-spec f(othermod:t()) -> ok.\nf(_) -> ok.\n");
    assert!(got.is_empty(), "unexpected: {got:?}");
}

// A multi-line -spec stays a type-attribute region across lines.
#[test]
fn a_multi_line_spec_is_skipped() {
    let got = calls("-spec f(X) -> ok when\n      X :: othermod:t().\nf(_) -> ok.\n");
    assert!(got.is_empty(), "unexpected: {got:?}");
}

// A variable-module call (`Mod:f(...)`) is dynamic dispatch, not a
// statically-named call.
#[test]
fn a_variable_module_call_is_skipped() {
    let got = calls("f(Mod) -> Mod:go(x).\n");
    assert!(got.is_empty(), "unexpected: {got:?}");
}
