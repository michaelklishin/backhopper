// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::envelope_version::{
    CURRENT_SCHEMA_VERSION as CORE_CURRENT, embedded_versions as core_embedded,
};
use backhopper_driver::{CURRENT_ENVELOPE_VERSION, supported_envelope_versions};

#[test]
fn supported_envelope_versions_ends_at_current() {
    let v = supported_envelope_versions();
    assert_eq!(v.last().copied(), Some(CURRENT_ENVELOPE_VERSION));
}

#[test]
fn supported_envelope_versions_starts_at_one() {
    let v = supported_envelope_versions();
    assert_eq!(v.first().copied(), Some(1));
}

#[test]
fn supported_envelope_versions_is_ascending_contiguous() {
    let v = supported_envelope_versions();
    for w in v.windows(2) {
        assert_eq!(w[0] + 1, w[1], "non-contiguous at {w:?}");
    }
}

#[test]
fn driver_re_exports_match_core_constants() {
    assert_eq!(CURRENT_ENVELOPE_VERSION, CORE_CURRENT);
    assert_eq!(supported_envelope_versions(), core_embedded());
}
