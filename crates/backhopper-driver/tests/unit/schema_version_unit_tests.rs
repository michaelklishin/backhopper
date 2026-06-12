// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_driver::{
    CURRENT_ENVELOPE_VERSION, SUPPORTED_SCHEMA, SchemaVersion, SchemaVersionRange,
};

#[test]
fn get_returns_underlying_integer() {
    assert_eq!(SchemaVersion::new(7).get(), 7);
}

#[test]
fn display_writes_the_integer() {
    assert_eq!(format!("{}", SchemaVersion::new(3)), "3");
}

#[test]
fn saturating_next_caps_at_u32_max() {
    let max = SchemaVersion::new(u32::MAX);
    assert_eq!(max.saturating_next(), max);
    assert_eq!(
        SchemaVersion::new(5).saturating_next(),
        SchemaVersion::new(6)
    );
}

#[test]
fn range_contains_bounds_inclusive() {
    let r = SchemaVersionRange {
        min: SchemaVersion::new(1),
        max: SchemaVersion::new(3),
    };
    assert!(r.contains(SchemaVersion::new(1)));
    assert!(r.contains(SchemaVersion::new(2)));
    assert!(r.contains(SchemaVersion::new(3)));
    assert!(!r.contains(SchemaVersion::new(0)));
    assert!(!r.contains(SchemaVersion::new(4)));
}

#[test]
fn singleton_is_a_one_element_range() {
    let r = SchemaVersionRange::singleton(SchemaVersion::new(2));
    assert!(r.contains(SchemaVersion::new(2)));
    assert!(!r.contains(SchemaVersion::new(1)));
}

#[test]
fn supported_schema_covers_version_one() {
    assert!(SUPPORTED_SCHEMA.contains(SchemaVersion::new(1)));
}

#[test]
fn supported_schema_covers_every_embedded_version() {
    for v in 1..=CURRENT_ENVELOPE_VERSION {
        assert!(
            SUPPORTED_SCHEMA.contains(SchemaVersion::new(v)),
            "v{v} must be inside SUPPORTED_SCHEMA: {SUPPORTED_SCHEMA}",
        );
    }
}

// guards against a refactor back to a hand-maintained literal
#[test]
fn supported_schema_ceiling_is_the_core_current_version() {
    assert_eq!(SUPPORTED_SCHEMA.max.get(), CURRENT_ENVELOPE_VERSION);
}

#[test]
fn supported_schema_rejects_the_next_version_and_beyond() {
    assert!(!SUPPORTED_SCHEMA.contains(SchemaVersion::new(CURRENT_ENVELOPE_VERSION + 1)));
    assert!(!SUPPORTED_SCHEMA.contains(SchemaVersion::new(999)));
}

#[test]
fn range_display_uses_inclusive_form() {
    let r = SchemaVersionRange::singleton(SchemaVersion::new(2));
    assert_eq!(format!("{r}"), "2..=2");
}
