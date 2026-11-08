// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Config, ConfigFile, Project};
use backhopper_core::model::names::TagName;
use backhopper_core::model::release_version::{
    HostSeries, HostTag, HostVersion, OtpLineVersion, OtpTag, OtpVersion, ReleaseVersion,
};

fn project(name: &str, prefix: &str) -> Project {
    let raw: ConfigFile = toml::from_str(&format!(
        r#"
config_version = 1
[[project]]
name = "{name}"
git_url = "/path/to/{name}.git"
tag_prefix = "{prefix}"
"#
    ))
    .unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
        .unwrap()
        .projects
        .remove(0)
}

fn tag(value: &str) -> TagName {
    TagName::new(value).unwrap()
}

#[test]
fn release_version_parses_host_and_otp_versions() {
    for value in ["4.2.8", "27.3.4.18", "29.1"] {
        let version = ReleaseVersion::parse(value).unwrap();
        assert_eq!(version.to_string(), value);
    }
}

#[test]
fn release_version_refuses_what_is_not_a_release() {
    for value in ["4.2.0-rc.1", "4.x", "4", "", "1.2.3.4.5.6.7", "4..2"] {
        assert!(ReleaseVersion::parse(value).is_err(), "{value:?}");
    }
}

#[test]
fn release_version_orders_a_shorter_list_before_its_extension() {
    let ordered =
        ["27.3", "27.3.0", "27.3.4.18", "27.4"].map(|v| ReleaseVersion::parse(v).unwrap());
    assert!(ordered.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn host_version_reports_its_series() {
    let version = HostVersion::parse("4.2.11").unwrap();
    assert_eq!(version.series(), "4.2".parse::<HostSeries>().unwrap());
    assert_eq!(version.series().to_string(), "4.2");
}

#[test]
fn host_series_refuses_anything_but_major_and_minor() {
    for value in ["4", "4.2.1", "v4.2", "4.x", ""] {
        assert!(value.parse::<HostSeries>().is_err(), "{value:?}");
    }
}

#[test]
fn otp_version_reports_its_major_and_line() {
    let version = OtpVersion::parse("27.3.4.18").unwrap();
    assert_eq!(version.major().get(), 27);
    assert_eq!(version.line(), "27.3".parse::<OtpLineVersion>().unwrap());
}

#[test]
fn otp_version_refuses_a_major_beyond_u16() {
    assert!(OtpVersion::parse("70000.1").is_err());
}

#[test]
fn host_tag_strips_the_project_prefix() {
    let tag = HostTag::parse(&project("rabbitmq-server", "v"), tag("v4.2.8")).unwrap();
    assert_eq!(tag.version().to_string(), "4.2.8");
    assert_eq!(tag.tag().as_str(), "v4.2.8");
}

#[test]
fn host_tag_refuses_a_pre_release_and_a_foreign_prefix() {
    let host = project("rabbitmq-server", "v");
    assert!(HostTag::parse(&host, tag("v4.2.0-rc.1")).is_err());
    assert!(HostTag::parse(&host, tag("4.2.0")).is_err());
}

#[test]
fn otp_tag_accepts_a_patch_tag_and_refuses_legacy_tags() {
    let otp = project("otp", "OTP-");
    let parsed = OtpTag::parse(&otp, tag("OTP-27.3.4.18")).unwrap();
    assert_eq!(parsed.version().to_string(), "27.3.4.18");
    assert!(OtpTag::parse(&otp, tag("OTP-17.0-rc1")).is_err());
    assert!(OtpTag::parse(&otp, tag("OTP_R16B03")).is_err());
}
