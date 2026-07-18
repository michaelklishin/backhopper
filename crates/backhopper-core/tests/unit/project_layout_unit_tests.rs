// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::config::ProjectLayout;

#[test]
fn single_app_defaults_are_empty() {
    // SingleApp carries no policy defaults: scan_paths comes from the workspace [defaults] section.
    let defaults = ProjectLayout::SingleApp.defaults();
    assert!(defaults.app_roots.is_empty());
    assert!(defaults.exclude_apps.is_empty());
    assert!(defaults.tag_pattern.is_none());
    assert!(defaults.min_tag.is_none());
}

#[test]
fn multi_app_defaults_are_empty() {
    let defaults = ProjectLayout::MultiApp.defaults();
    assert!(defaults.app_roots.is_empty());
    assert!(defaults.exclude_apps.is_empty());
    assert!(defaults.excluded_subdirs.is_empty());
    assert!(defaults.tag_pattern.is_none());
    assert!(defaults.min_tag.is_none());
}

#[test]
fn erlang_otp_defaults_carry_rpm_derived_policy() {
    let defaults = ProjectLayout::ErlangOtp.defaults();
    assert_eq!(defaults.app_roots, vec!["lib/*", "erts/preloaded"]);
    let excluded: Vec<&str> = defaults.exclude_apps.iter().map(|a| a.as_str()).collect();
    for expected in [
        "odbc",
        "snmp",
        "ssh",
        "tftp",
        "ftp",
        "wx",
        "megaco",
        "edoc",
        "jinterface",
        "diameter",
    ] {
        assert!(
            excluded.contains(&expected),
            "expected {expected} in exclude_apps"
        );
    }
    assert_eq!(
        defaults.excluded_subdirs,
        vec!["doc", "example", "examples", "test"]
    );
    assert_eq!(defaults.tag_pattern.as_ref().unwrap().as_str(), "OTP-*");
    assert_eq!(defaults.min_tag.as_ref().unwrap().as_str(), "OTP-26.0");
}

#[test]
fn from_str_accepts_three_layouts() {
    assert_eq!(
        ProjectLayout::from_str("single_app").unwrap(),
        ProjectLayout::SingleApp
    );
    assert_eq!(
        ProjectLayout::from_str("multi_app").unwrap(),
        ProjectLayout::MultiApp
    );
    assert_eq!(
        ProjectLayout::from_str("erlang_otp").unwrap(),
        ProjectLayout::ErlangOtp
    );
}

#[test]
fn from_str_rejects_unknown_value() {
    assert!(ProjectLayout::from_str("erlang").is_err());
    assert!(ProjectLayout::from_str("OTP").is_err());
    assert!(ProjectLayout::from_str("").is_err());
}

#[test]
fn as_str_round_trips_through_from_str() {
    for layout in [
        ProjectLayout::SingleApp,
        ProjectLayout::MultiApp,
        ProjectLayout::ErlangOtp,
    ] {
        let s = layout.as_str();
        assert_eq!(ProjectLayout::from_str(s).unwrap(), layout);
    }
}
