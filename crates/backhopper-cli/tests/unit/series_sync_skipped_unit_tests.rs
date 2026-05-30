// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Language, Project, ProjectFamily, ProjectKind, ProjectLayout};
use backhopper_core::model::names::{ProjectName, SeriesName};

use backhopper_cli::commands::series::{
    SkippedPin, SyncOutput, build_sync_output, build_sync_output_for_branch,
    render_sync_text_with_options,
};

fn project(name: &str, tag_prefix: &str) -> Project {
    Project {
        name: ProjectName::new(name).unwrap(),
        git_url: Some(PathBuf::from(format!("/tmp/{name}.git"))),
        kind: ProjectKind::External,
        family: ProjectFamily::Generic,
        language: Language::Erlang,
        tag_prefix: tag_prefix.into(),
        public_modules: Vec::new(),
        internal_modules: Vec::new(),
        layout: ProjectLayout::SingleApp,
        scan_paths: Vec::new(),
        app_roots: Vec::new(),
        include_apps: Vec::new(),
        exclude_apps: Vec::new(),
        excluded_subdirs: Vec::new(),
        tag_pattern: None,
        min_tag: None,
        exclude_tag_markers: Vec::new(),
    }
}

fn series(name: &str) -> SeriesName {
    SeriesName::new(name).unwrap()
}

fn render(output: &SyncOutput, show_skipped: bool) -> String {
    let mut buf = Vec::new();
    render_sync_text_with_options(output, &mut buf, show_skipped).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn dep_name_starting_with_digit_is_recorded_as_skipped() {
    let mk = "dep_3lib = hex 1.0.0\n";
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &[]);
    assert!(out.pins.is_empty());
    assert!(out.dropped_unconfigured.is_empty());
    assert_eq!(
        out.skipped,
        vec![SkippedPin {
            name: "3lib".into(),
            reason: "name not a valid project identifier".into(),
        }]
    );
}

#[test]
fn unconfigured_dep_is_dropped_not_skipped() {
    let mk = "dep_unknown = hex 1.0.0\n";
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &[]);
    assert!(out.skipped.is_empty());
    assert_eq!(out.dropped_unconfigured, vec!["unknown".to_string()]);
}

#[test]
fn build_sync_output_records_branch_when_provided() {
    let mk = "dep_ra = hex 2.16.13\n";
    let projects = vec![project("ra", "v")];
    let out = build_sync_output_for_branch(mk, &series("rabbitmq-4.1"), &projects, Some("v4.1.x"));
    assert_eq!(out.branch.as_deref(), Some("v4.1.x"));
}

#[test]
fn build_sync_output_leaves_branch_unset_for_default_constructor() {
    let mk = "dep_ra = hex 2.16.13\n";
    let projects = vec![project("ra", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects);
    assert!(out.branch.is_none());
}

#[test]
fn render_with_show_skipped_emits_skipped_lines() {
    let out = SyncOutput {
        name: "rabbitmq-4.1".into(),
        branch: None,
        pins: Vec::new(),
        dropped_unconfigured: Vec::new(),
        skipped: vec![SkippedPin {
            name: "weird".into(),
            reason: "tag not valid".into(),
        }],
    };
    let s = render(&out, true);
    assert!(s.contains("# skipped weird: tag not valid"), "{s}");
}

#[test]
fn render_without_show_skipped_omits_skipped_lines() {
    let out = SyncOutput {
        name: "rabbitmq-4.1".into(),
        branch: None,
        pins: Vec::new(),
        dropped_unconfigured: Vec::new(),
        skipped: vec![SkippedPin {
            name: "weird".into(),
            reason: "anything".into(),
        }],
    };
    let s = render(&out, false);
    assert!(!s.contains("# skipped"), "{s}");
}

#[test]
fn render_with_branch_includes_inferred_from_comment() {
    let out = SyncOutput {
        name: "rabbitmq-4.2".into(),
        branch: Some("v4.2.x".into()),
        pins: Vec::new(),
        dropped_unconfigured: Vec::new(),
        skipped: Vec::new(),
    };
    let s = render(&out, false);
    assert!(s.contains("# inferred from v4.2.x"), "{s}");
}

#[test]
fn skipped_pins_are_sorted_alphabetically_by_name() {
    let mk = "dep_3z = hex 1.0.0\ndep_3a = hex 1.0.0\ndep_3m = hex 1.0.0\n";
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &[]);
    let names: Vec<_> = out.skipped.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["3a", "3m", "3z"]);
}

#[test]
fn mix_of_valid_skipped_dropped_is_partitioned_correctly() {
    let mk = "\
dep_ra = hex 2.16.13
dep_3bad = hex 1.0.0
dep_unknown = hex 0.0.1
";
    let projects = vec![project("ra", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects);
    assert_eq!(out.pins.len(), 1);
    assert_eq!(out.pins[0].project, "ra");
    assert_eq!(out.dropped_unconfigured, vec!["unknown".to_string()]);
    assert_eq!(out.skipped.len(), 1);
    assert_eq!(out.skipped[0].name, "3bad");
}
