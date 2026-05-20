// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang::deprecated::parse_deprecation_attribute;

#[test]
fn parses_simple_function_arity_pair() {
    let v = parse_deprecation_attribute("([{f, 1}])");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].function.as_deref(), Some("f"));
    assert_eq!(v[0].arity, Some(1));
    assert!(!v[0].any_arity);
}

#[test]
fn parses_function_arity_with_string_reason() {
    let v = parse_deprecation_attribute(r#"([{f, 1, "since X"}])"#);
    assert_eq!(v[0].reason.as_deref(), Some("since X"));
}

#[test]
fn parses_any_arity_pattern() {
    let v = parse_deprecation_attribute("([{f, '_'}])");
    assert_eq!(v[0].function.as_deref(), Some("f"));
    assert!(v[0].any_arity);
}

#[test]
fn parses_module_wide() {
    let v = parse_deprecation_attribute("(module)");
    assert!(v[0].module_wide);
}

#[test]
fn parses_atom_reason() {
    let v = parse_deprecation_attribute("([{f, 1, next_major_release}])");
    assert_eq!(v[0].reason.as_deref(), Some("next_major_release"));
}
