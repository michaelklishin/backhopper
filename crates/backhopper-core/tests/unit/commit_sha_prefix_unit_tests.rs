// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::errors::NameError;
use backhopper_core::model::names::CommitShaPrefix;

#[test]
fn accepts_seven_through_forty_lowercase_hex() {
    for len in [7usize, 8, 10, 20, 39, 40] {
        let value = "a".repeat(len);
        let p = CommitShaPrefix::new(value.clone()).expect("valid");
        assert_eq!(p.len(), len);
        assert_eq!(p.as_str(), value.as_str());
        assert_eq!(p.is_full(), len == 40);
    }
}

#[test]
fn rejects_below_seven_characters() {
    for raw in ["", "a", "abcdef"] {
        let err = CommitShaPrefix::new(raw).expect_err("too short");
        assert!(matches!(
            err,
            NameError::EmptyCommitShaPrefix | NameError::CommitShaPrefixTooShort { .. }
        ));
    }
}

#[test]
fn rejects_above_forty_characters() {
    let raw = "a".repeat(41);
    let err = CommitShaPrefix::new(raw).expect_err("too long");
    assert!(matches!(
        err,
        NameError::CommitShaPrefixTooLong { len: 41, .. }
    ));
}

#[test]
fn rejects_non_hex_characters() {
    let err = CommitShaPrefix::new("abcz1234").expect_err("non hex");
    match err {
        NameError::CommitShaPrefixNonHex { ch, position, .. } => {
            assert_eq!(ch, 'z');
            assert_eq!(position, 3);
        }
        other => panic!("expected NonHex, got {other:?}"),
    }
}

#[test]
fn normalises_mixed_case_to_lowercase() {
    let p = CommitShaPrefix::new("ABCDEF1").expect("valid after lowercase");
    assert_eq!(p.as_str(), "abcdef1");
}

#[test]
fn is_full_true_only_at_forty_chars() {
    assert!(!CommitShaPrefix::new("a".repeat(7)).unwrap().is_full());
    assert!(!CommitShaPrefix::new("a".repeat(39)).unwrap().is_full());
    assert!(CommitShaPrefix::new("a".repeat(40)).unwrap().is_full());
}

#[test]
fn as_full_sha_returns_some_only_at_forty_chars() {
    assert!(
        CommitShaPrefix::new("a".repeat(8))
            .unwrap()
            .as_full_sha()
            .is_none()
    );
    let full = CommitShaPrefix::new("a".repeat(40)).unwrap();
    assert!(full.as_full_sha().is_some());
    assert_eq!(full.as_full_sha().unwrap().as_str(), &"a".repeat(40));
}

#[test]
fn from_str_round_trips_through_string() {
    let p: CommitShaPrefix = "abc1234".parse().expect("valid");
    assert_eq!(p.as_str(), "abc1234");
    assert_eq!(p.to_string(), "abc1234");
}

#[test]
fn from_str_normalises_case() {
    let p: CommitShaPrefix = "ABC1234".parse().expect("valid mixed case");
    assert_eq!(p.as_str(), "abc1234");
}

#[test]
fn serde_round_trip_emits_bare_string() {
    let p = CommitShaPrefix::new("c6f799ab0e").unwrap();
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, "\"c6f799ab0e\"");
    let parsed: CommitShaPrefix = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, p);
}

#[test]
fn serde_normalises_uppercase_on_deserialise() {
    let p: CommitShaPrefix = serde_json::from_str("\"ABC1234\"").expect("normalise then accept");
    assert_eq!(p.as_str(), "abc1234");
}

#[test]
fn serde_rejects_too_short_prefix() {
    assert!(serde_json::from_str::<CommitShaPrefix>("\"abc\"").is_err());
}

#[test]
fn serde_rejects_non_hex_prefix() {
    assert!(serde_json::from_str::<CommitShaPrefix>("\"abcdezzz\"").is_err());
}

#[test]
fn from_str_empty_returns_empty_variant() {
    assert!(matches!(
        CommitShaPrefix::from_str(""),
        Err(NameError::EmptyCommitShaPrefix)
    ));
}

#[test]
fn as_ref_str_yields_canonical_form() {
    let p = CommitShaPrefix::new("Abc1234").unwrap();
    let r: &str = p.as_ref();
    assert_eq!(r, "abc1234");
}

#[test]
fn try_from_string_runs_the_validator() {
    let ok = CommitShaPrefix::try_from("abc1234".to_owned()).unwrap();
    assert_eq!(ok.as_str(), "abc1234");
    assert!(CommitShaPrefix::try_from("zzz".to_owned()).is_err());
}

#[test]
fn into_string_yields_canonical_lowercase() {
    let p = CommitShaPrefix::new("ABCDEF1").unwrap();
    let s: String = p.into();
    assert_eq!(s, "abcdef1");
}

#[test]
fn ambiguous_sha_display_uses_truncated_at_count() {
    let err = backhopper_core::errors::GitError::AmbiguousSha {
        prefix: "abc1234".into(),
        candidates: vec!["abc12340000000000000000000000000000aaaaa".into()],
        truncated_at: 12,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("12 objects"),
        "expected truncated_at count, got: {msg}"
    );
}

#[test]
fn ambiguous_sha_display_uses_singular_form_for_one_match() {
    let err = backhopper_core::errors::GitError::AmbiguousSha {
        prefix: "abc1234".into(),
        candidates: vec!["abc12340000000000000000000000000000aaaaa".into()],
        truncated_at: 1,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("1 object,") || msg.ends_with("1 object"),
        "expected singular form, got: {msg}"
    );
}
