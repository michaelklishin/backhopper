// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Validation invariants for `RelativePath`: any constructed value
//! is non-empty, neither absolute nor parent-escaping, and round-trips
//! cleanly through serde.

use proptest::prelude::*;

use backhopper_core::model::names::RelativePath;

fn safe_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,8}".prop_map(String::from)
}

fn safe_relative_path() -> impl Strategy<Value = String> {
    proptest::collection::vec(safe_segment(), 1..6).prop_map(|segs| segs.join("/"))
}

proptest! {
    #[test]
    fn safe_paths_construct_and_display(s in safe_relative_path()) {
        let p = RelativePath::new(s.clone()).expect("safe path is valid");
        prop_assert_eq!(p.as_str(), &s);
        prop_assert_eq!(p.to_string(), s);
    }

    #[test]
    fn safe_paths_roundtrip_via_json(s in safe_relative_path()) {
        let p = RelativePath::new(s).expect("safe path is valid");
        let json = serde_json::to_string(&p).expect("serialise");
        let back: RelativePath = serde_json::from_str(&json).expect("deserialise");
        prop_assert_eq!(back, p);
    }

    #[test]
    fn absolute_paths_always_rejected(suffix in safe_relative_path()) {
        let raw = format!("/{}", suffix);
        prop_assert!(RelativePath::new(raw).is_err());
    }

    #[test]
    fn dot_dot_segments_always_rejected(prefix in safe_segment(), suffix in safe_segment()) {
        let raw = format!("{}/../{}", prefix, suffix);
        prop_assert!(RelativePath::new(raw).is_err());
    }

    #[test]
    fn nul_bytes_always_rejected(seg in safe_segment()) {
        let raw = format!("{}\0bad", seg);
        prop_assert!(RelativePath::new(raw).is_err());
    }

    #[test]
    fn from_bytes_round_trips_when_path_is_valid(s in safe_relative_path()) {
        let bytes = s.as_bytes();
        let p = RelativePath::from_bytes(bytes).expect("safe utf-8 bytes parse");
        prop_assert_eq!(p.as_str(), &s);
    }

    #[test]
    fn from_bytes_rejects_invalid_utf8(prefix in safe_segment()) {
        let mut raw = prefix.into_bytes();
        raw.push(0x80);
        raw.push(0xC0);
        prop_assert!(RelativePath::from_bytes(&raw).is_err());
    }
}
