// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::model::names::{CommitSha, CommitShaPrefix};

const HEX: &str = "0123456789abcdef";
const HEX_MIXED: &str = "0123456789abcdefABCDEF";

fn any_hex_string(min_len: usize, max_len: usize) -> impl Strategy<Value = String> {
    (min_len..=max_len).prop_flat_map(|n| {
        proptest::collection::vec(prop::sample::select(HEX.as_bytes()), n)
            .prop_map(|chars| String::from_utf8(chars).expect("ascii"))
    })
}

fn any_mixed_case_hex(min_len: usize, max_len: usize) -> impl Strategy<Value = String> {
    (min_len..=max_len).prop_flat_map(|n| {
        proptest::collection::vec(prop::sample::select(HEX_MIXED.as_bytes()), n)
            .prop_map(|chars| String::from_utf8(chars).expect("ascii"))
    })
}

fn forty_char_hex() -> impl Strategy<Value = String> {
    any_hex_string(40, 40)
}

proptest! {
    #[test]
    fn any_7_to_40_lowercase_hex_round_trips(s in any_hex_string(7, 40)) {
        let p = CommitShaPrefix::new(s.clone()).expect("valid");
        prop_assert_eq!(p.as_str(), s.as_str());
        prop_assert_eq!(p.len(), s.len());
        prop_assert_eq!(p.is_full(), s.len() == 40);
    }

    #[test]
    fn any_mixed_case_hex_normalises_to_lowercase(s in any_mixed_case_hex(7, 40)) {
        let p = CommitShaPrefix::new(s.clone()).expect("valid after normalisation");
        let lower = s.to_ascii_lowercase();
        prop_assert_eq!(p.as_str(), lower.as_str());
    }

    #[test]
    fn full_form_prefix_can_be_lifted_to_commit_sha(s in forty_char_hex()) {
        let p = CommitShaPrefix::new(s.clone()).unwrap();
        prop_assert!(p.is_full());
        let lifted: CommitSha = p.as_full_sha().expect("full");
        prop_assert_eq!(lifted.as_str(), s.as_str());
    }

    #[test]
    fn short_prefix_never_lifts_to_full_sha(s in any_hex_string(7, 39)) {
        let p = CommitShaPrefix::new(s).unwrap();
        prop_assert!(p.as_full_sha().is_none());
    }

    #[test]
    fn rejects_non_hex_characters(s in "[g-zG-Z]{7,40}") {
        prop_assert!(CommitShaPrefix::new(s).is_err());
    }

    #[test]
    fn rejects_too_short(s in any_hex_string(1, 6)) {
        prop_assert!(CommitShaPrefix::new(s).is_err());
    }

    #[test]
    fn rejects_too_long(s in any_hex_string(41, 80)) {
        prop_assert!(CommitShaPrefix::new(s).is_err());
    }

    #[test]
    fn serde_json_round_trip_preserves_value(s in any_hex_string(7, 40)) {
        let p = CommitShaPrefix::new(s).unwrap();
        let wire = serde_json::to_string(&p).unwrap();
        let back: CommitShaPrefix = serde_json::from_str(&wire).unwrap();
        prop_assert_eq!(p, back);
    }
}
