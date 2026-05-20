// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang::records::parse_record;

#[test]
fn parses_simple_record() {
    let r = parse_record("(state, {name, age})").unwrap();
    assert_eq!(r.name, "state");
    assert_eq!(r.fields.len(), 2);
    assert_eq!(r.fields[0].name, "name");
    assert_eq!(r.fields[0].type_repr, None);
    assert_eq!(r.fields[1].name, "age");
}

#[test]
fn parses_record_with_typed_fields() {
    let r = parse_record("(cfg, {id :: ra_server_id(), uid :: ra_uid()})").unwrap();
    assert_eq!(r.name, "cfg");
    assert_eq!(r.fields.len(), 2);
    assert_eq!(r.fields[0].type_repr.as_deref(), Some("ra_server_id()"));
    assert_eq!(r.fields[1].type_repr.as_deref(), Some("ra_uid()"));
}

#[test]
fn parses_record_with_default_and_type() {
    let r = parse_record("(s, {x :: integer() | undefined = 0})").unwrap();
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "x");
    assert_eq!(
        r.fields[0].type_repr.as_deref(),
        Some("integer() | undefined")
    );
}

#[test]
fn parses_record_without_fields() {
    let r = parse_record("(empty, {})").unwrap();
    assert_eq!(r.name, "empty");
    assert!(r.fields.is_empty());
}

#[test]
fn parses_record_field_with_default_before_type() {
    let r = parse_record("(s, {x = 0 :: integer()})").unwrap();
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "x");
    assert_eq!(r.fields[0].type_repr.as_deref(), Some("integer()"));
}

#[test]
fn parses_record_field_with_only_default() {
    let r = parse_record("(s, {x = 7})").unwrap();
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "x");
    assert_eq!(r.fields[0].type_repr, None);
}
