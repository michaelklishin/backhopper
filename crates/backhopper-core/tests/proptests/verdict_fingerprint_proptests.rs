// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use proptest::prelude::*;

use backhopper_core::model::fingerprint::VerdictFingerprint;

const HEX: &str = "0123456789abcdef";

fn any_hex_string(len: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(prop::sample::select(HEX.as_bytes()), len)
        .prop_map(|chars| String::from_utf8(chars).expect("ascii"))
}

/// A 32-character hex string with an uppercase letter forced into the
/// first position: guaranteed invalid, unlike a random `[A-F0-9]{32}`
/// string, which may land on an all-digit value that lowercase hex
/// would also accept.
fn uppercase_hex() -> impl Strategy<Value = String> {
    (prop::sample::select(&b"ABCDEF"[..]), any_hex_string(31))
        .prop_map(|(first, rest)| format!("{}{rest}", first as char))
}

fn any_other_shape() -> impl Strategy<Value = String> {
    prop_oneof![
        any_hex_string(31),
        any_hex_string(33),
        uppercase_hex(),
        "[g-zG-Z]{32}",
    ]
}

proptest! {
    #[test]
    fn from_digest_then_display_then_from_str_is_identity(digest: [u8; 16]) {
        let fp = VerdictFingerprint::from_digest(digest);
        let text = fp.to_string();
        prop_assert_eq!(text.len(), 32);
        let back = VerdictFingerprint::from_str(&text).expect("from_digest output round-trips");
        prop_assert_eq!(fp, back);
    }

    #[test]
    fn any_valid_hex_round_trips(s in any_hex_string(32)) {
        let fp = VerdictFingerprint::try_from(s.clone()).expect("valid");
        prop_assert_eq!(fp.as_str(), s.as_str());
    }

    #[test]
    fn wrong_length_case_or_charset_is_refused(s in any_other_shape()) {
        prop_assert!(VerdictFingerprint::try_from(s).is_err());
    }
}
