// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Config, ConfigFile, ProjectLayout, Series};
use backhopper_core::errors::ConfigError;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::{PinSelect, PinSpec};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
use backhopper_core::store::SnapshotStore;
use tempfile::TempDir;
use time::OffsetDateTime;

fn parse(toml: &str) -> Result<Config, ConfigError> {
    let raw: ConfigFile = toml::from_str(toml).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

#[test]
fn erlang_otp_layout_fills_in_rpm_defaults_when_nothing_else_set() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "otp"
git_url = "/path/to/otp.git"
layout = "erlang_otp"
"#,
    )
    .unwrap();
    let p = &cfg.projects[0];
    assert_eq!(p.layout, ProjectLayout::ErlangOtp);
    assert_eq!(p.app_roots, vec!["lib/*", "erts/preloaded"]);
    assert!(p.exclude_apps.iter().any(|a| a.as_str() == "wx"));
    assert!(p.exclude_apps.iter().any(|a| a.as_str() == "diameter"));
    assert!(!p.exclude_apps.iter().any(|a| a.as_str() == "stdlib"));
    assert_eq!(
        p.excluded_subdirs,
        vec!["doc", "example", "examples", "test"]
    );
    assert_eq!(p.tag_pattern.as_ref().unwrap().as_str(), "OTP-*");
    assert_eq!(p.min_tag.as_ref().unwrap().as_str(), "OTP-26.0");
}

#[test]
fn erlang_otp_user_overrides_replace_the_default_exclude_list() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "otp"
git_url = "/p.git"
layout = "erlang_otp"
exclude_apps = ["wx"]
"#,
    )
    .unwrap();
    let names: Vec<&str> = cfg.projects[0]
        .exclude_apps
        .iter()
        .map(|a| a.as_str())
        .collect();
    assert_eq!(names, vec!["wx"]);
}

#[test]
fn multi_app_layout_without_app_roots_is_a_hard_error() {
    let err = parse(
        r#"
config_version = 1
[[project]]
name = "monorepo"
git_url = "/m.git"
layout = "multi_app"
"#,
    )
    .unwrap_err();
    match err {
        ConfigError::LayoutWithoutAppRoots { project, layout } => {
            assert_eq!(project, "monorepo");
            assert_eq!(layout, "multi_app");
        }
        other => panic!("expected LayoutWithoutAppRoots, got {other:?}"),
    }
}

#[test]
fn single_app_default_layout_when_not_specified() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/r.git"
"#,
    )
    .unwrap();
    assert_eq!(cfg.projects[0].layout, ProjectLayout::SingleApp);
    assert!(cfg.projects[0].app_roots.is_empty());
    assert!(!cfg.projects[0].scan_paths.is_empty());
}

#[test]
fn unknown_layout_value_is_an_error() {
    let err = parse(
        r#"
config_version = 1
[[project]]
name = "x"
git_url = "/x.git"
layout = "umbrella"
"#,
    )
    .unwrap_err();
    match err {
        ConfigError::UnknownProjectLayout(value) => assert_eq!(value, "umbrella"),
        other => panic!("expected UnknownProjectLayout, got {other:?}"),
    }
}

#[test]
fn literal_pin_parses_to_pinspec_literal() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/r.git"
[[series]]
name = "stable"
pins = [{ project = "ra", tag = "v2.16.13" }]
"#,
    )
    .unwrap();
    match &cfg.series[0].pins[0] {
        PinSpec::Literal { project, tag } => {
            assert_eq!(project.as_str(), "ra");
            assert_eq!(tag.as_str(), "v2.16.13");
        }
        PinSpec::Pattern { .. } => panic!("expected literal, got pattern"),
    }
}

#[test]
fn pattern_pin_parses_to_pinspec_pattern() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "otp"
git_url = "/o.git"
layout = "erlang_otp"
[[series]]
name = "rabbitmq-4.1"
pins = [{ project = "otp", tag_pattern = "OTP-26.*", select = "latest" }]
"#,
    )
    .unwrap();
    match &cfg.series[0].pins[0] {
        PinSpec::Pattern {
            project,
            pattern,
            select,
        } => {
            assert_eq!(project.as_str(), "otp");
            assert_eq!(pattern.as_str(), "OTP-26.*");
            assert!(matches!(select, PinSelect::Latest));
        }
        PinSpec::Literal { .. } => panic!("expected pattern, got literal"),
    }
}

#[test]
fn series_can_carry_two_pins_for_the_same_project() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "otp"
git_url = "/o.git"
layout = "erlang_otp"
[[series]]
name = "rabbitmq-4.1"
pins = [
    { project = "otp", tag_pattern = "OTP-26.*", select = "latest" },
    { project = "otp", tag_pattern = "OTP-27.*", select = "latest" },
]
"#,
    )
    .unwrap();
    assert_eq!(cfg.series[0].pins.len(), 2);
    assert_eq!(cfg.series[0].pins[0].project().as_str(), "otp");
    assert_eq!(cfg.series[0].pins[1].project().as_str(), "otp");
}

#[test]
fn pin_select_must_be_latest_or_oldest() {
    let err = parse(
        r#"
config_version = 1
[[project]]
name = "otp"
git_url = "/o.git"
layout = "erlang_otp"
[[series]]
name = "s"
pins = [{ project = "otp", tag_pattern = "OTP-26.*", select = "newest" }]
"#,
    )
    .unwrap_err();
    match err {
        ConfigError::PinUnknownSelect(value) => assert_eq!(value, "newest"),
        other => panic!("expected PinUnknownSelect, got {other:?}"),
    }
}

#[test]
fn single_app_inherits_workspace_scan_paths_when_project_does_not_set_them() {
    let cfg = parse(
        r#"
config_version = 1
[defaults]
scan_paths = ["custom/**/*.erl"]
[[project]]
name = "ra"
git_url = "/r.git"
"#,
    )
    .unwrap();
    assert_eq!(cfg.projects[0].scan_paths, vec!["custom/**/*.erl"]);
}

#[test]
fn multi_app_layouts_do_not_inherit_workspace_scan_paths() {
    let cfg = parse(
        r#"
config_version = 1
[defaults]
scan_paths = ["custom/**/*.erl"]
[[project]]
name = "otp"
git_url = "/o.git"
layout = "erlang_otp"
"#,
    )
    .unwrap();
    assert!(cfg.projects[0].scan_paths.is_empty());
}

#[test]
fn series_resolve_pins_walks_specs_against_store() {
    let project = ProjectName::new("ra").unwrap();
    let tmp = TempDir::new().unwrap();
    let mut_store = SnapshotStore::open_mut(tmp.path()).unwrap();
    for tag_str in ["v2.16.13", "v2.17.0"] {
        let header = SnapshotHeader {
            project: project.clone(),
            tag: TagName::new(tag_str).unwrap(),
            branch: None,
            commit: CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: Vec::new(),
            apps_scanned: Vec::new(),
            generated_by: "backhopper".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        };
        mut_store
            .write(&Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical())
            .unwrap();
    }
    let store = SnapshotStore::open(tmp.path().to_path_buf()).unwrap();
    let series = Series {
        name: SeriesName::new("s").unwrap(),
        pins: vec![PinSpec::literal(
            project.clone(),
            TagName::new("v2.16.13").unwrap(),
        )],
    };
    let pins = series.resolve_pins(&store).unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].tag.as_str(), "v2.16.13");
}

#[test]
fn default_exclude_tag_markers_cover_common_prerelease_suffixes() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/r.git"
"#,
    )
    .unwrap();
    let markers = &cfg.projects[0].exclude_tag_markers;
    assert!(markers.iter().any(|m| m == "-rc"));
    assert!(markers.iter().any(|m| m == "-alpha"));
    assert!(markers.iter().any(|m| m == "-beta"));
    assert!(markers.iter().any(|m| m == "-pre"));
}

#[test]
fn user_can_opt_in_to_prerelease_tags_with_empty_list() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/r.git"
exclude_tag_markers = []
"#,
    )
    .unwrap();
    assert!(cfg.projects[0].exclude_tag_markers.is_empty());
}

#[test]
fn user_can_extend_prerelease_marker_list() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/r.git"
exclude_tag_markers = ["-rc", "-nightly"]
"#,
    )
    .unwrap();
    assert_eq!(cfg.projects[0].exclude_tag_markers, vec!["-rc", "-nightly"]);
}

#[test]
fn is_prerelease_tag_detects_marker_substring() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/r.git"
"#,
    )
    .unwrap();
    let p = &cfg.projects[0];
    assert!(p.is_prerelease_tag(&TagName::new("OTP-26.0-rc1").unwrap()));
    assert!(p.is_prerelease_tag(&TagName::new("v1.0.0-alpha").unwrap()));
    assert!(!p.is_prerelease_tag(&TagName::new("OTP-26.0").unwrap()));
    assert!(!p.is_prerelease_tag(&TagName::new("v1.2.3").unwrap()));
}

#[test]
fn user_can_narrow_tag_pattern_and_min_tag_on_erlang_otp() {
    let cfg = parse(
        r#"
config_version = 1
[[project]]
name = "otp"
git_url = "/o.git"
layout = "erlang_otp"
tag_pattern = "OTP-27.*"
min_tag = "OTP-27.0"
"#,
    )
    .unwrap();
    assert_eq!(
        cfg.projects[0].tag_pattern.as_ref().unwrap().as_str(),
        "OTP-27.*"
    );
    assert_eq!(
        cfg.projects[0].min_tag.as_ref().unwrap().as_str(),
        "OTP-27.0"
    );
}
