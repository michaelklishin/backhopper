// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use proptest::prelude::*;

use backhopper_cache::{canonical_json, content_hash};

proptest! {
    #[test]
    fn canonical_json_is_deterministic(map in prop::collection::btree_map(
        "[a-z]{1,8}", any::<i64>(), 0..8,
    )) {
        let a = canonical_json(&map).unwrap();
        let b = canonical_json(&map).unwrap();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn hash_ignores_map_insertion_order(map in prop::collection::btree_map(
        "[a-z]{1,8}", any::<i64>(), 0..8,
    )) {
        let pairs: Vec<(String, i64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let reversed: BTreeMap<String, i64> = pairs.into_iter().rev().collect();
        prop_assert_eq!(content_hash(&map).unwrap(), content_hash(&reversed).unwrap());
    }
}
