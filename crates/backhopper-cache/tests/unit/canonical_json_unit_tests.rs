// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use serde::Serialize;

use backhopper_cache::{canonical_json, content_hash};

#[derive(Serialize)]
struct AlphaFirst {
    alpha: u32,
    beta: &'static str,
}

#[derive(Serialize)]
struct BetaFirst {
    beta: &'static str,
    alpha: u32,
}

#[test]
fn canonical_json_sorts_object_keys() {
    let bytes = canonical_json(&BetaFirst {
        beta: "x",
        alpha: 7,
    })
    .unwrap();
    assert_eq!(bytes, br#"{"alpha":7,"beta":"x"}"#);
}

#[test]
fn field_declaration_order_does_not_change_the_hash() {
    let a = content_hash(&AlphaFirst {
        alpha: 7,
        beta: "x",
    })
    .unwrap();
    let b = content_hash(&BetaFirst {
        beta: "x",
        alpha: 7,
    })
    .unwrap();
    assert_eq!(a, b);
}

#[test]
fn different_values_hash_differently() {
    let a = content_hash(&("series-a", ["main"])).unwrap();
    let b = content_hash(&("series-b", ["main"])).unwrap();
    assert_ne!(a, b);
}

#[test]
fn hash_is_lowercase_hex_of_fixed_length() {
    let h = content_hash(&"anything").unwrap();
    assert_eq!(h.len(), 32);
    assert!(
        h.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );
}
