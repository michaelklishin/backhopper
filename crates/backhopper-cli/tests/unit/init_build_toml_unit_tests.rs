// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Tests for `init::build_toml`: shape, parseability, round-trip into
//! `Config::load`-compatible TOML.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backhopper_core::config::Config;
use backhopper_core::model::names::{ProjectName, SeriesName, TagName};

use backhopper_cli::commands::init::{
    InferredPin, InferredProject, InferredSeries, InferredWorkspace, build_toml,
};

fn pn(s: &str) -> ProjectName {
    ProjectName::new(s).unwrap()
}
fn tn(s: &str) -> TagName {
    TagName::new(s).unwrap()
}
fn sn(s: &str) -> SeriesName {
    SeriesName::new(s).unwrap()
}

fn empty_workspace() -> InferredWorkspace {
    InferredWorkspace::default()
}

#[test]
fn empty_workspace_produces_defaults_block_only() {
    let body = build_toml(&PathBuf::from("/tmp/snap"), &empty_workspace());
    assert!(body.starts_with("config_version = 1"));
    assert!(body.contains("[defaults]"));
    assert!(body.contains("/tmp/snap"));
    // Look for the literal table header on its own line; the empty-state
    // comment `# [[project]]` would otherwise trip a substring check.
    assert!(
        !body.lines().any(|l| l.trim() == "[[project]]"),
        "no real [[project]] block expected"
    );
    assert!(!body.lines().any(|l| l.trim() == "[[series]]"));
    assert!(body.contains("# Add one [[project]]"));
}

#[test]
fn empty_workspace_output_is_valid_toml_and_loads_as_config() {
    let body = build_toml(&PathBuf::from("/tmp/snap"), &empty_workspace());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("backhopper.toml");
    std::fs::write(&path, &body).unwrap();
    let cfg = Config::load(&path).expect("starter config must load");
    assert!(cfg.projects.is_empty());
    assert!(cfg.series.is_empty());
}

#[test]
fn workspace_with_projects_and_series_round_trips() {
    let mut projects: BTreeMap<ProjectName, InferredProject> = BTreeMap::new();
    projects.insert(
        pn("ra"),
        InferredProject {
            name: pn("ra"),
            tag_prefix: "v".into(),
        },
    );
    projects.insert(
        pn("khepri"),
        InferredProject {
            name: pn("khepri"),
            tag_prefix: "v".into(),
        },
    );
    let series = vec![InferredSeries {
        name: sn("rabbitmq-4.1"),
        branch: "v4.1.x".into(),
        pins: vec![
            InferredPin {
                project: pn("ra"),
                tag: tn("v2.16.13"),
            },
            InferredPin {
                project: pn("khepri"),
                tag: tn("v0.17.0"),
            },
        ],
    }];
    let workspace = InferredWorkspace {
        projects,
        series,
        skipped: vec![],
    };
    let body = build_toml(&PathBuf::from("/tmp/snap"), &workspace);

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("backhopper.toml");
    std::fs::write(&path, &body).unwrap();
    let cfg = Config::load(&path).expect("config must round-trip");
    assert_eq!(cfg.projects.len(), 2);
    assert_eq!(cfg.series.len(), 1);
    assert_eq!(cfg.series[0].name.as_str(), "rabbitmq-4.1");
    assert_eq!(cfg.series[0].pins.len(), 2);
}

#[test]
fn duplicate_series_names_are_de_duplicated() {
    let series = vec![
        InferredSeries {
            name: sn("rabbitmq-4.1"),
            branch: "v4.1.x".into(),
            pins: vec![],
        },
        InferredSeries {
            name: sn("rabbitmq-4.1"),
            branch: "main".into(),
            pins: vec![],
        },
    ];
    let workspace = InferredWorkspace {
        projects: BTreeMap::new(),
        series,
        skipped: vec![],
    };
    let body = build_toml(&PathBuf::from("/tmp/snap"), &workspace);
    let count = body.matches("[[series]]").count();
    assert_eq!(
        count, 1,
        "second occurrence of same series name must be skipped"
    );
}

#[test]
fn project_block_includes_required_git_url_placeholder() {
    let mut projects: BTreeMap<ProjectName, InferredProject> = BTreeMap::new();
    projects.insert(
        pn("ra"),
        InferredProject {
            name: pn("ra"),
            tag_prefix: "v".into(),
        },
    );
    let workspace = InferredWorkspace {
        projects,
        series: vec![],
        skipped: vec![],
    };
    let body = build_toml(&PathBuf::from("/tmp/snap"), &workspace);
    assert!(body.contains("git_url"));
    assert!(body.contains("TODO"));
    assert!(
        !body.contains("# git_url"),
        "git_url must NOT be a comment; required field"
    );
}
