// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backhopper_core::config::{Language, Project, ProjectLayout};
use backhopper_core::model::names::{ProjectName, SeriesName};

use backhopper_cli::commands::series::{
    PinPayload, SyncOutput, build_sync_output, render_sync_text,
};

fn project(name: &str, tag_prefix: &str) -> Project {
    Project {
        name: ProjectName::new(name).unwrap(),
        git_url: PathBuf::from(format!("/tmp/{name}.git")),
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

fn render(output: &SyncOutput) -> String {
    let mut buf = Vec::new();
    render_sync_text(output, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn hex_dep_takes_project_tag_prefix() {
    let mk = "dep_ra = hex 2.16.13\n";
    let projects = vec![project("ra", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects).unwrap();
    assert_eq!(
        out.pins,
        vec![PinPayload {
            project: "ra".into(),
            tag: "v2.16.13".into()
        }]
    );
}

#[test]
fn git_dep_uses_version_verbatim_as_tag() {
    let mk = "dep_osiris = git https://github.com/rabbitmq/osiris v1.8.8\n";
    let projects = vec![project("osiris", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects).unwrap();
    assert_eq!(
        out.pins,
        vec![PinPayload {
            project: "osiris".into(),
            tag: "v1.8.8".into()
        }]
    );
}

#[test]
fn git_rmq_dep_uses_version_verbatim_as_tag() {
    let mk = "dep_cowboy = git_rmq cowboy 2.13.0\n";
    let projects = vec![project("cowboy", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects).unwrap();
    assert_eq!(
        out.pins,
        vec![PinPayload {
            project: "cowboy".into(),
            tag: "2.13.0".into()
        }]
    );
}

#[test]
fn deps_with_no_configured_project_go_to_dropped() {
    let mk = "dep_ra = hex 2.16.13\ndep_jose = hex 1.11.12\ndep_unknown = hex 0.0.1\n";
    let projects = vec![project("ra", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects).unwrap();
    assert_eq!(out.pins.len(), 1);
    assert_eq!(out.pins[0].project, "ra");
    assert_eq!(out.dropped_unconfigured, vec!["jose", "unknown"]);
}

#[test]
fn pins_and_dropped_are_alphabetically_sorted() {
    let mk = "dep_z_dep = hex 1.0.0\ndep_a_dep = hex 2.0.0\ndep_m_unknown = hex 0.0.1\n";
    let projects = vec![project("a_dep", "v"), project("z_dep", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects).unwrap();
    assert_eq!(
        out.pins
            .iter()
            .map(|p| p.project.as_str())
            .collect::<Vec<_>>(),
        vec!["a_dep", "z_dep"]
    );
    assert_eq!(out.dropped_unconfigured, vec!["m_unknown"]);
}

#[test]
fn rabbitmq_4_2_x_components_mk_produces_five_known_pins() {
    let mk = "\
dep_amqp_client = git_rmq amqp_client 4.2.0
dep_ra = hex 2.17.3
dep_osiris = git https://github.com/rabbitmq/osiris v1.10.3
dep_khepri = hex 0.17.7
dep_khepri_mnesia_migration = hex 0.8.1
dep_seshat = hex 1.0.1
dep_jose = hex 1.11.12
";
    let projects = vec![
        project("ra", "v"),
        project("osiris", "v"),
        project("khepri", "v"),
        project("khepri_mnesia_migration", "v"),
        project("seshat", "v"),
    ];
    let out = build_sync_output(mk, &series("rabbitmq-4.2"), &projects).unwrap();
    let by_project: BTreeMap<_, _> = out
        .pins
        .iter()
        .map(|p| (p.project.clone(), p.tag.clone()))
        .collect();
    assert_eq!(by_project["ra"], "v2.17.3");
    assert_eq!(by_project["osiris"], "v1.10.3");
    assert_eq!(by_project["khepri"], "v0.17.7");
    assert_eq!(by_project["khepri_mnesia_migration"], "v0.8.1");
    assert_eq!(by_project["seshat"], "v1.0.1");
    assert_eq!(out.dropped_unconfigured, vec!["amqp_client", "jose"]);
}

#[test]
fn empty_mk_produces_empty_payload() {
    let out = build_sync_output("", &series("rabbitmq-4.1"), &[]).unwrap();
    assert!(out.pins.is_empty());
    assert!(out.dropped_unconfigured.is_empty());
    assert_eq!(out.name, "rabbitmq-4.1");
}

#[test]
fn text_renders_toml_stanza_with_aligned_tags() {
    let out = SyncOutput {
        name: "rabbitmq-4.1".into(),
        pins: vec![
            PinPayload {
                project: "ra".into(),
                tag: "v2.16.13".into(),
            },
            PinPayload {
                project: "khepri".into(),
                tag: "v0.16.0".into(),
            },
        ],
        dropped_unconfigured: Vec::new(),
    };
    let s = render(&out);
    assert!(
        s.starts_with("[[series]]\nname = \"rabbitmq-4.1\"\n"),
        "{s}"
    );
    assert!(s.contains("pins = ["));
    assert!(s.contains("project = \"ra\""));
    assert!(s.contains("tag = \"v2.16.13\""));
}

#[test]
fn text_renders_dropped_as_trailing_comment_block() {
    let out = SyncOutput {
        name: "rabbitmq-4.1".into(),
        pins: Vec::new(),
        dropped_unconfigured: vec!["cowboy".into(), "ranch".into()],
    };
    let s = render(&out);
    assert!(s.contains("pins = []"));
    assert!(s.contains("# dropped"));
    assert!(s.contains("#   cowboy"));
    assert!(s.contains("#   ranch"));
}

#[test]
fn text_omits_dropped_block_when_no_drops() {
    let out = SyncOutput {
        name: "x".into(),
        pins: vec![PinPayload {
            project: "a".into(),
            tag: "v1.0.0".into(),
        }],
        dropped_unconfigured: Vec::new(),
    };
    let s = render(&out);
    assert!(!s.contains("# dropped"));
}

#[test]
fn hex_dep_with_already_prefixed_version_is_not_double_prefixed() {
    let mk = "dep_ra = hex v3.0.0\n";
    let projects = vec![project("ra", "v")];
    let out = build_sync_output(mk, &series("rabbitmq-4.1"), &projects).unwrap();
    assert_eq!(out.pins[0].tag, "v3.0.0");
}

#[test]
fn hex_dep_with_empty_tag_prefix_keeps_bare_version() {
    let mk = "dep_jose = hex 1.11.12\n";
    let projects = vec![project("jose", "")];
    let out = build_sync_output(mk, &series("s"), &projects).unwrap();
    assert_eq!(out.pins[0].tag, "1.11.12");
}
