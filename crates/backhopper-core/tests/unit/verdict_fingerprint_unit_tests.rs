// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::model::fingerprint::VerdictFingerprint;

#[test]
fn from_digest_formats_thirty_two_lowercase_hex_characters() {
    let fp = VerdictFingerprint::from_digest([0xab; 16]);
    assert_eq!(fp.as_str(), "ab".repeat(16));
}

#[test]
fn a_valid_hex_string_is_accepted() {
    let hex = "0123456789abcdef0123456789abcdef";
    assert!(VerdictFingerprint::try_from(hex[..32].to_owned()).is_ok());
}

#[test]
fn an_uppercase_hex_string_is_refused() {
    let hex = "AB".repeat(16);
    assert!(VerdictFingerprint::try_from(hex).is_err());
}

#[test]
fn a_short_string_is_refused() {
    assert!(VerdictFingerprint::try_from("ab".repeat(15)).is_err());
}

#[test]
fn a_long_string_is_refused() {
    assert!(VerdictFingerprint::try_from("ab".repeat(17)).is_err());
}

#[test]
fn a_non_hex_string_is_refused() {
    assert!(VerdictFingerprint::try_from("g".repeat(32)).is_err());
}

#[test]
fn from_str_delegates_to_try_from() {
    let fp = VerdictFingerprint::from_str(&"ab".repeat(16)).unwrap();
    assert_eq!(fp.as_str(), "ab".repeat(16));
    assert!(VerdictFingerprint::from_str("not hex").is_err());
}

#[test]
fn serde_reads_through_try_from() {
    let json = serde_json::Value::String("ab".repeat(16));
    let fp: VerdictFingerprint = serde_json::from_value(json).unwrap();
    assert_eq!(fp.as_str(), "ab".repeat(16));

    let bad = serde_json::Value::String("not hex".to_owned());
    assert!(serde_json::from_value::<VerdictFingerprint>(bad).is_err());
}
