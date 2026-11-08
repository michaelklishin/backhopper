// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::release_version::{ErlangVersionRange, OtpLineVersion};

fn line(value: &str) -> OtpLineVersion {
    value.parse().unwrap()
}

#[test]
fn half_open_range_contains_its_lower_bound_and_not_its_upper() {
    let range: ErlangVersionRange = "[27.0,30.0)".parse().unwrap();
    assert!(range.contains(line("27.0")));
    assert!(range.contains(line("29.1")));
    assert!(!range.contains(line("30.0")));
    assert!(!range.contains(line("26.2")));
}

#[test]
fn a_range_ending_at_a_major_excludes_the_minors_after_it() {
    let range: ErlangVersionRange = "[27.0,29.0)".parse().unwrap();
    assert!(!range.contains(line("29.1")));
    assert!(range.contains(line("28.5")));
}

#[test]
fn a_range_starting_at_a_minor_excludes_earlier_minors_of_the_same_major() {
    let range: ErlangVersionRange = "[26.2,28.0)".parse().unwrap();
    assert!(range.contains(line("26.2")));
    assert!(!range.contains(line("26.1")));
}

#[test]
fn notation_other_than_half_open_is_refused() {
    for value in [
        "(27.0,30.0]",
        "[30.0,27.0)",
        "[27.0,27.0)",
        "27.0..30.0",
        "[27.0,30.0]",
        "(27.0,30.0)",
        "[27.0)",
        "",
    ] {
        assert!(value.parse::<ErlangVersionRange>().is_err(), "{value:?}");
    }
}

#[test]
fn serde_refuses_what_from_str_refuses() {
    for value in ["(27.0,30.0]", "[30.0,27.0)", "27.0..30.0"] {
        let json = serde_json::to_string(value).unwrap();
        assert!(serde_json::from_str::<ErlangVersionRange>(&json).is_err());
    }
    let range: ErlangVersionRange = serde_json::from_str("\"[27.0,30.0)\"").unwrap();
    assert_eq!(serde_json::to_string(&range).unwrap(), "\"[27.0,30.0)\"");
}
