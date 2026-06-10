// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Version-like tag string comparison.

use std::cmp::Ordering;

/// Compares two version-like tag strings in *descending* order: numerically
/// newer comes first. Pairs with `min_by`/`max_by` callers that want the
/// "latest" or "oldest" tag.
#[must_use]
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let a_parts = parse_version(a);
    let b_parts = parse_version(b);
    a_parts.cmp(&b_parts).reverse()
}

fn parse_version(tag: &str) -> Vec<u64> {
    let stripped = tag.strip_prefix('v').unwrap_or(tag);
    stripped
        .split(['.', '-', '+', '_'])
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}
