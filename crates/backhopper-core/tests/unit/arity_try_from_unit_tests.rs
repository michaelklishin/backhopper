// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::Arity;

#[test]
fn try_from_usize_accepts_the_full_u8_range() {
    assert_eq!(Arity::try_from(0usize).unwrap().get(), 0);
    assert_eq!(Arity::try_from(255usize).unwrap().get(), 255);
}

#[test]
fn try_from_usize_rejects_above_the_arity_ceiling() {
    assert!(Arity::try_from(256usize).is_err());
    assert!(Arity::try_from(usize::MAX).is_err());
}
