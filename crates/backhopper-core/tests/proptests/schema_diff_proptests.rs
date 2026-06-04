// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants of the schema-diff walker over arbitrary JSON values.

use backhopper_core::schema_diff::diff;
use proptest::prelude::*;
use serde_json::{Value, json};

fn arb_scalar() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!(null)),
        any::<bool>().prop_map(|b| json!(b)),
        any::<i64>().prop_map(|n| json!(n)),
        "[a-z]{0,8}".prop_map(|s| json!(s)),
    ]
}

fn arb_flat_object() -> impl Strategy<Value = Value> {
    proptest::collection::btree_map("[a-z]{1,4}", arb_scalar(), 0..6).prop_map(|m| {
        let map: serde_json::Map<String, Value> = m.into_iter().collect();
        Value::Object(map)
    })
}

proptest! {
    #[test]
    fn identical_values_yield_no_differences(v in arb_flat_object()) {
        let d = diff(1, 2, &v, &v);
        prop_assert!(d.added_paths.is_empty());
        prop_assert!(d.removed_paths.is_empty());
        prop_assert!(d.changed_types.is_empty());
    }

    #[test]
    fn diff_is_antisymmetric_on_added_removed(a in arb_flat_object(), b in arb_flat_object()) {
        let ab = diff(1, 2, &a, &b);
        let ba = diff(2, 1, &b, &a);
        prop_assert_eq!(ab.added_paths, ba.removed_paths);
        prop_assert_eq!(ab.removed_paths, ba.added_paths);
        prop_assert_eq!(ab.changed_types.len(), ba.changed_types.len());
    }
}
