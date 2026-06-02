// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::io::Cursor;

use backhopper_driver::StdinPayload;

#[test]
fn default_is_none() {
    let p = StdinPayload::default();
    assert!(p.is_none());
}

#[test]
fn bytes_payload_is_not_none() {
    let bytes = vec![0u8, 1, 2];
    let p = StdinPayload::Bytes(&bytes);
    assert!(!p.is_none());
}

#[test]
fn debug_summarises_byte_count() {
    let bytes = vec![0u8; 5];
    let p = StdinPayload::Bytes(&bytes);
    let s = format!("{p:?}");
    assert!(s.contains("5"), "debug should mention byte count, got {s}");
}

#[test]
fn debug_for_reader_does_not_panic() {
    let mut reader = Cursor::new(vec![1u8, 2, 3]);
    let p = StdinPayload::Reader(&mut reader);
    let _ = format!("{p:?}");
    assert!(!p.is_none());
}
