// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::CommitSha;

#[test]
fn abbreviated_returns_first_twelve_hex_chars() {
    let sha = CommitSha::new("abcdef0123456789abcdef0123456789abcdef01").unwrap();
    assert_eq!(sha.abbreviated(), "abcdef012345");
    assert_eq!(sha.abbreviated().len(), 12);
}

#[test]
fn abbreviated_borrows_the_underlying_string() {
    let sha = CommitSha::new("0".repeat(40)).unwrap();
    let s1 = sha.abbreviated();
    let s2 = sha.abbreviated();
    assert!(std::ptr::eq(s1.as_ptr(), s2.as_ptr()));
}
