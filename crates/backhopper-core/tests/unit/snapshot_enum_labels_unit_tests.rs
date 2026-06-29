// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

// The writer and the parser share one label table per enum, so a label
// that round-trips here cannot drift between the two sides.

use backhopper_core::model::snapshot::{
    IfdefGuardKind, TestExportVariant, VendoredDepSource, Visibility,
};

#[test]
fn visibility_keyword_round_trips() {
    for v in [Visibility::Public, Visibility::Hidden, Visibility::TestOnly] {
        assert_eq!(Visibility::from_keyword(v.keyword()), Some(v));
    }
    assert_eq!(Visibility::from_keyword("internal"), None);
}

#[test]
fn test_export_variant_label_round_trips() {
    for v in [TestExportVariant::A, TestExportVariant::B] {
        assert_eq!(TestExportVariant::from_label(v.as_label()), Some(v));
    }
    assert_eq!(TestExportVariant::from_label("c"), None);
}

#[test]
fn ifdef_guard_kind_label_round_trips() {
    for v in [
        IfdefGuardKind::Test,
        IfdefGuardKind::NotTest,
        IfdefGuardKind::Other,
    ] {
        assert_eq!(IfdefGuardKind::from_label(v.as_label()), Some(v));
    }
    assert_eq!(IfdefGuardKind::from_label("maybe"), None);
}

#[test]
fn vendored_dep_source_label_round_trips() {
    for v in [
        VendoredDepSource::Hex,
        VendoredDepSource::Git,
        VendoredDepSource::GitRmq,
    ] {
        assert_eq!(VendoredDepSource::from_label(v.as_label()), Some(v));
    }
    assert_eq!(VendoredDepSource::from_label("svn"), None);
}
