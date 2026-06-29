// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::verdict::SeriesSummary;

fn summary(
    compatible: u32,
    requires_adaptation: u32,
    incompatible: u32,
    inapplicable: u32,
) -> SeriesSummary {
    SeriesSummary {
        compatible,
        requires_adaptation,
        incompatible,
        inapplicable,
    }
}

#[test]
fn incompatible_or_requires_adaptation_blocks() {
    assert!(summary(0, 0, 1, 0).is_blocking());
    assert!(summary(0, 1, 0, 0).is_blocking());
    assert!(summary(3, 1, 2, 0).is_blocking());
}

#[test]
fn only_compatible_or_inapplicable_does_not_block() {
    assert!(!summary(0, 0, 0, 0).is_blocking());
    assert!(!summary(5, 0, 0, 0).is_blocking());
    assert!(!summary(0, 0, 0, 4).is_blocking());
    assert!(!summary(5, 0, 0, 4).is_blocking());
}
