// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use backhopper_core::compat::option_keys::{OptionKeySet, drifted_keys, parse_option_key_set};
use backhopper_core::model::names::MapKey;

fn key(s: &str) -> MapKey {
    MapKey::new(s).unwrap()
}

#[test]
fn closed_map_with_required_and_optional_keys() {
    let set = parse_option_key_set("#{name := atom(), idle_timeout => integer()}");
    let OptionKeySet::Closed(keys) = set else {
        panic!("expected closed set, got {set:?}");
    };
    assert_eq!(keys, BTreeSet::from([key("name"), key("idle_timeout")]));
}

#[test]
fn quoted_atom_key_counts_as_literal() {
    let set = parse_option_key_set("#{'data_delivery' => atom()}");
    let OptionKeySet::Closed(keys) = set else {
        panic!("expected closed set, got {set:?}");
    };
    assert_eq!(keys, BTreeSet::from([key("data_delivery")]));
}

#[test]
fn wildcard_key_marks_the_set_open() {
    let set = parse_option_key_set("#{atom() => any()}");
    assert!(matches!(set, OptionKeySet::Open(_)));
}

#[test]
fn open_set_keeps_its_literal_keys() {
    let set = parse_option_key_set("#{name := atom(), atom() => any()}");
    let OptionKeySet::Open(keys) = set else {
        panic!("expected open set, got {set:?}");
    };
    assert_eq!(keys, BTreeSet::from([key("name")]));
}

#[test]
fn union_type_is_unresolved() {
    let set = parse_option_key_set("#{a := b()} | undefined");
    assert_eq!(set, OptionKeySet::Unresolved);
}

#[test]
fn indirection_to_another_type_is_unresolved() {
    let set = parse_option_key_set("ra_server_id()");
    assert_eq!(set, OptionKeySet::Unresolved);
}

#[test]
fn nested_map_member_keys_do_not_leak_to_top_level() {
    let set = parse_option_key_set("#{outer := #{inner := atom()}}");
    let OptionKeySet::Closed(keys) = set else {
        panic!("expected closed set, got {set:?}");
    };
    assert_eq!(keys, BTreeSet::from([key("outer")]));
}

#[test]
fn drift_is_source_minus_target_when_target_is_closed() {
    let source = OptionKeySet::Closed(BTreeSet::from([key("a"), key("b")]));
    let target = OptionKeySet::Closed(BTreeSet::from([key("a")]));
    assert_eq!(drifted_keys(&source, &target), BTreeSet::from([key("b")]));
}

#[test]
fn open_target_suppresses_drift_entirely() {
    let source = OptionKeySet::Closed(BTreeSet::from([key("a"), key("b")]));
    let target = OptionKeySet::Open(BTreeSet::from([key("a")]));
    assert!(drifted_keys(&source, &target).is_empty());
}

#[test]
fn open_source_still_diffs_its_literal_keys() {
    let source = OptionKeySet::Open(BTreeSet::from([key("a"), key("b")]));
    let target = OptionKeySet::Closed(BTreeSet::from([key("a")]));
    assert_eq!(drifted_keys(&source, &target), BTreeSet::from([key("b")]));
}

#[test]
fn parser_never_panics_on_arbitrary_text() {
    for s in ["", "#{", "#{}", "}}}", "#{,,,}", "#{'unterminated", "#{=>}"] {
        let _ = parse_option_key_set(s);
    }
}
