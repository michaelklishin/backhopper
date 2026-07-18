// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::suites::rules::validate_template_placeholders;

#[test]
fn validate_passes_for_known_placeholders() {
    let allowed = vec!["plugin".to_owned(), "dir".to_owned()];
    assert!(validate_template_placeholders("{plugin}_SUITE", &allowed).is_ok());
    assert!(validate_template_placeholders("{dir}_{plugin}_SUITE", &allowed).is_ok());
    assert!(validate_template_placeholders("no_placeholders", &allowed).is_ok());
}

#[test]
fn validate_rejects_unknown_placeholders() {
    let allowed = vec!["plugin".to_owned()];
    let err = validate_template_placeholders("{plugin}_{typo}", &allowed).unwrap_err();
    assert_eq!(err.placeholder, "typo");
}

#[test]
fn validate_ignores_unbalanced_brace_at_end() {
    let allowed = vec!["plugin".to_owned()];
    // The current grammar treats a trailing `{` with no closing `}` as literal.
    assert!(validate_template_placeholders("{plugin}_trailing{", &allowed).is_ok());
}
