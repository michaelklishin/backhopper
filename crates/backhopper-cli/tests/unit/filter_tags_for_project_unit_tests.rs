// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Language, Project, ProjectFamily, ProjectKind, ProjectLayout};
use backhopper_core::model::names::{ProjectName, TagGlob, TagName};

use backhopper_cli::commands::snapshots::filter_tags_for_project;

fn project_with(tag_pattern: Option<&str>, min_tag: Option<&str>) -> Project {
    Project {
        name: ProjectName::new("otp").unwrap(),
        git_url: Some(PathBuf::from("/tmp/otp.git")),
        kind: ProjectKind::External,
        family: ProjectFamily::Generic,
        language: Language::Erlang,
        tag_prefix: "v".into(),
        public_modules: Vec::new(),
        internal_modules: Vec::new(),
        layout: ProjectLayout::ErlangOtp,
        scan_paths: Vec::new(),
        app_roots: vec!["lib/*".into()],
        include_apps: Vec::new(),
        exclude_apps: Vec::new(),
        excluded_subdirs: Vec::new(),
        tag_pattern: tag_pattern.map(|s| TagGlob::new(s).unwrap()),
        min_tag: min_tag.map(|s| TagName::new(s).unwrap()),
        exclude_tag_markers: Vec::new(),
    }
}

fn tags(strs: &[&str]) -> Vec<TagName> {
    strs.iter().map(|s| TagName::new(*s).unwrap()).collect()
}

#[test]
fn tag_pattern_keeps_only_matching_tags() {
    let p = project_with(Some("OTP-*"), None);
    let input = tags(&["OTP-26.0", "OTP-27.0", "R16B03", "v1.0"]);
    let out = filter_tags_for_project(input, &p, None);
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, vec!["OTP-26.0", "OTP-27.0"]);
}

#[test]
fn min_tag_drops_older_versions() {
    let p = project_with(None, Some("OTP-26.0"));
    let input = tags(&["OTP-25.0", "OTP-26.0", "OTP-26.2.5", "OTP-27.0", "R16B03"]);
    let out = filter_tags_for_project(input, &p, None);
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    // R16B03 has no numeric OTP version pieces: the invariant we check is that older OTP versions are dropped
    assert!(!names.contains(&"OTP-25.0"));
    assert!(names.contains(&"OTP-26.0"));
    assert!(names.contains(&"OTP-26.2.5"));
    assert!(names.contains(&"OTP-27.0"));
}

#[test]
fn tag_pattern_and_min_tag_stack() {
    let p = project_with(Some("OTP-*"), Some("OTP-26.0"));
    let input = tags(&[
        "OTP-25.0",
        "OTP-26.0",
        "OTP-26.2.5",
        "OTP-27.0",
        "R16B03",
        "v1.0",
    ]);
    let out = filter_tags_for_project(input, &p, None);
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, vec!["OTP-26.0", "OTP-26.2.5", "OTP-27.0"]);
}

#[test]
fn cli_since_stacks_on_top_of_project_filters() {
    let p = project_with(Some("OTP-*"), Some("OTP-26.0"));
    let input = tags(&["OTP-26.0", "OTP-26.2.5", "OTP-27.0"]);
    let since = TagName::new("OTP-26.2.5").unwrap();
    let out = filter_tags_for_project(input, &p, Some(&since));
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    // --since means at or after, version-sorted: keep 26.2.5 and 27.0, drop 26.0
    assert_eq!(names, vec!["OTP-26.2.5", "OTP-27.0"]);
}

#[test]
fn no_filters_is_identity() {
    let p = project_with(None, None);
    let input = tags(&["OTP-26.0", "v1.0", "R16B03"]);
    let out = filter_tags_for_project(input.clone(), &p, None);
    assert_eq!(out, input);
}

#[test]
fn exclude_tag_markers_drops_prerelease_tags() {
    let mut p = project_with(None, None);
    p.exclude_tag_markers = vec!["-rc".into(), "-alpha".into()];
    let input = tags(&[
        "OTP-26.0",
        "OTP-26.0-rc1",
        "OTP-26.0-rc2",
        "OTP-26.0-alpha",
        "OTP-26.2.5",
    ]);
    let out = filter_tags_for_project(input, &p, None);
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, vec!["OTP-26.0", "OTP-26.2.5"]);
}

#[test]
fn empty_exclude_tag_markers_keeps_prerelease_tags() {
    let mut p = project_with(None, None);
    p.exclude_tag_markers = Vec::new();
    let input = tags(&["OTP-26.0", "OTP-26.0-rc1"]);
    let out = filter_tags_for_project(input, &p, None);
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, vec!["OTP-26.0", "OTP-26.0-rc1"]);
}

#[test]
fn all_four_filters_stack_for_otp_style_config() {
    let mut p = project_with(Some("OTP-*"), Some("OTP-26.0"));
    p.exclude_tag_markers = vec!["-rc".into()];
    let input = tags(&[
        "OTP-25.0",
        "OTP-26.0",
        "OTP-26.0-rc1",
        "OTP-26.2.5",
        "OTP-27.0",
        "OTP-27.0-rc2",
        "R16B03",
        "v1.0",
    ]);
    let since = TagName::new("OTP-26.2.5").unwrap();
    let out = filter_tags_for_project(input, &p, Some(&since));
    let names: Vec<&str> = out.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, vec!["OTP-26.2.5", "OTP-27.0"]);
}
