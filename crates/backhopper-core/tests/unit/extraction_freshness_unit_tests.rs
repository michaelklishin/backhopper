// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::config::{Config, ConfigFile, Project};
use backhopper_core::model::app_manifest::ManifestSource;
use backhopper_core::model::names::{
    ApplicationName, ExtractorVersion, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::FORMAT_VERSION;
use backhopper_core::snapshot::freshness::{
    ExpectedExtraction, StaleReason, extraction_freshness, extraction_settings_digest,
};
use backhopper_test_support::snapshot_header;

const CONFIG: &str = r#"
config_version = 1

[[project]]
name = "rabbitmq-server"
git_url = "https://github.com/rabbitmq/rabbitmq-server.git"
layout = "multi_app"
app_roots = ["deps/*"]
"#;

fn project() -> Project {
    let raw: ConfigFile = toml::from_str(CONFIG).unwrap();
    let cfg = Config::from_raw("/tmp/backhopper.toml".into(), raw).unwrap();
    cfg.project(&ProjectName::new("rabbitmq-server").unwrap())
        .unwrap()
        .clone()
}

type Change = fn(&mut Project);

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

#[test]
fn every_setting_that_changes_extraction_changes_the_digest() {
    let base = extraction_settings_digest(&project());
    let changes: [(&str, Change); 8] = [
        ("manifest", |p| p.manifest = ManifestSource::ErlangMk),
        ("app_roots", |p| p.app_roots.push("plugins/*".into())),
        ("scan_paths", |p| p.scan_paths.push("extra/src".into())),
        ("include_apps", |p| p.include_apps.push(app("rabbit"))),
        ("exclude_apps", |p| {
            p.exclude_apps.push(app("rabbit_common"))
        }),
        ("excluded_subdirs", |p| {
            p.excluded_subdirs.push("priv".into())
        }),
        ("public_modules", |p| {
            p.public_modules
                .push(ModuleName::new("rabbit_misc").unwrap());
        }),
        ("internal_modules", |p| {
            p.internal_modules
                .push(ModuleName::new("rabbit_misc").unwrap());
        }),
    ];
    for (name, change) in changes {
        let mut changed = project();
        change(&mut changed);
        assert_ne!(extraction_settings_digest(&changed), base, "{name}");
    }
}

#[test]
fn a_setting_that_does_not_change_extraction_leaves_the_digest_alone() {
    let base = extraction_settings_digest(&project());
    let mut other = project();
    other.min_tag = Some(TagName::new("v4.0.0").unwrap());
    other.tag_prefix = "rabbit-".into();
    other.exclude_tag_markers.push("-beta".into());
    assert_eq!(extraction_settings_digest(&other), base);
}

#[test]
fn the_order_of_a_list_setting_does_not_change_the_digest() {
    let mut first = project();
    first.exclude_apps = vec![app("rabbit_common"), app("rabbit")];
    let mut second = project();
    second.exclude_apps = vec![app("rabbit"), app("rabbit_common"), app("rabbit")];
    assert_eq!(
        extraction_settings_digest(&first),
        extraction_settings_digest(&second)
    );
}

fn expected() -> ExpectedExtraction {
    ExpectedExtraction {
        extractor: ExtractorVersion::new("9").unwrap(),
        settings: extraction_settings_digest(&project()),
    }
}

fn current_header() -> backhopper_core::model::snapshot::SnapshotHeader {
    let mut header = snapshot_header("rabbitmq-server", "v4.2.0");
    header.format_version = FORMAT_VERSION;
    header.extractor_version = "9".into();
    header.extraction_settings = Some(extraction_settings_digest(&project()));
    header
}

#[test]
fn a_header_with_every_input_current_is_fresh() {
    assert_eq!(extraction_freshness(&current_header(), &expected()), None);
}

#[test]
fn an_older_extractor_is_stale_before_anything_else_is_compared() {
    let mut header = current_header();
    header.extractor_version = "8".into();
    header.format_version = 4;
    assert_eq!(
        extraction_freshness(&header, &expected()),
        Some(StaleReason::ExtractorVersion { found: "8".into() })
    );
}

#[test]
fn a_header_with_no_extractor_version_is_stale() {
    let mut header = current_header();
    header.extractor_version = String::new();
    assert_eq!(
        extraction_freshness(&header, &expected()),
        Some(StaleReason::ExtractorVersion {
            found: String::new()
        })
    );
}

#[test]
fn an_older_format_is_stale() {
    let mut header = current_header();
    header.format_version = 4;
    assert_eq!(
        extraction_freshness(&header, &expected()),
        Some(StaleReason::FormatVersion { found: 4 })
    );
}

#[test]
fn other_extraction_settings_are_stale() {
    let mut other = project();
    other.manifest = ManifestSource::ErlangMk;
    let mut header = current_header();
    header.extraction_settings = Some(extraction_settings_digest(&other));
    assert_eq!(
        extraction_freshness(&header, &expected()),
        Some(StaleReason::ExtractionSettings)
    );
}

#[test]
fn a_header_that_records_no_settings_is_stale() {
    let mut header = current_header();
    header.extraction_settings = None;
    assert_eq!(
        extraction_freshness(&header, &expected()),
        Some(StaleReason::ExtractionSettings)
    );
}

#[test]
fn each_reason_reads_as_a_plain_sentence() {
    assert_eq!(
        StaleReason::ExtractionSettings.to_string(),
        "made with other extraction settings than the configured ones"
    );
    assert_eq!(
        StaleReason::ExtractorVersion {
            found: String::new()
        }
        .to_string(),
        "no extractor version recorded"
    );
    assert_eq!(
        StaleReason::FormatVersion { found: 4 }.to_string(),
        format!("format-version 4, expected {FORMAT_VERSION}")
    );
}
