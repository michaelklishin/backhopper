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
fn record_field_with_bitstring_type_is_not_split() {
    // the comma inside <<_:8, _:_*8>> is part of the bitstring type, not a field separator
    let r = parse_record("(frame, {kind, payload :: <<_:8, _:_*8>>, version})").unwrap();
    assert_eq!(r.name, "frame");
    assert_eq!(r.fields.len(), 3);
    assert_eq!(r.fields[1].name, "payload");
    assert_eq!(r.fields[1].type_repr.as_deref(), Some("<<_:8, _:_*8>>"));
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
fn a_map_typed_field_keeps_its_arrow_intact() {
    // the `=` of `=>` is not a default separator
    let r = parse_record("(cfg, {overrides = #{a => b} :: map()})").unwrap();
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "overrides");
    assert_eq!(r.fields[0].type_repr.as_deref(), Some("map()"));
}

#[test]
fn a_map_type_with_no_default_survives_whole() {
    let r = parse_record("(cfg, {overrides :: #{atom() => term()}})").unwrap();
    assert_eq!(
        r.fields[0].type_repr.as_deref(),
        Some("#{atom() => term()}")
    );
}

#[test]
fn a_range_typed_field_parses() {
    let r = parse_record("(frame, {version :: 1..255})").unwrap();
    assert_eq!(r.fields[0].name, "version");
    assert_eq!(r.fields[0].type_repr.as_deref(), Some("1..255"));
}

#[test]
fn an_equals_inside_a_default_string_is_not_a_separator() {
    let r = parse_record("(cfg, {banner = \"a=b\"})").unwrap();
    assert_eq!(r.fields[0].name, "banner");
    assert_eq!(r.fields[0].type_repr, None);
}

#[test]
fn parses_record_field_with_only_default() {
    let r = parse_record("(s, {x = 7})").unwrap();
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "x");
    assert_eq!(r.fields[0].type_repr, None);
}

#[test]
fn record_fields_below_comment_lines_all_survive() {
    let record = parse_record(
        "(state, {%% the raft id\n id :: integer(),\n leader, % current leader\n voted_for})",
    )
    .expect("parsed");
    let names: Vec<&str> = record.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["id", "leader", "voted_for"]);
    assert_eq!(record.fields[0].type_repr.as_deref(), Some("integer()"));
}
