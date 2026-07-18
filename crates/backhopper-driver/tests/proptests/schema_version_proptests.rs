// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_driver::{SchemaVersion, SchemaVersionRange};
use proptest::prelude::*;

proptest! {
    #[test]
    fn serde_round_trip_preserves_value(v in proptest::num::u32::ANY) {
        let original = SchemaVersion::new(v);
        let s = serde_json::to_string(&original).unwrap();
        let back: SchemaVersion = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(original, back);
    }

    #[test]
    fn single_version_range_contains_only_its_value(v in 0u32..1_000_000) {
        let r = SchemaVersionRange {
            min: SchemaVersion::new(v),
            max: SchemaVersion::new(v),
        };
        prop_assert!(r.contains(SchemaVersion::new(v)));
        if v > 0 {
            prop_assert!(!r.contains(SchemaVersion::new(v - 1)));
        }
        if v < u32::MAX {
            prop_assert!(!r.contains(SchemaVersion::new(v + 1)));
        }
    }
}
