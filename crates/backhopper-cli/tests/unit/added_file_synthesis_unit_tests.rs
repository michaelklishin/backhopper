// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Unit-level checks for the CLI-side helpers that feed the 018
//! resolvers: `synthesise_added_file_content` and
//! `collect_search_path_globs`. The end-to-end flow is exercised in
//! the workspace-level integration tests through the `check` command.

use std::path::PathBuf;

use backhopper_cli::commands::target_repo::{
    collect_search_path_globs, synthesise_added_file_content,
};
use backhopper_core::compat::patch::{Hunk, HunkLine, Language, PatchedFile};
use backhopper_core::config::{
    Config, Language as ConfigLanguage, Project, ProjectFamily, ProjectKind, ProjectLayout,
};
use backhopper_core::model::names::ProjectName;

fn fully_added(path: &str, added_lines: &[&str]) -> PatchedFile {
    PatchedFile {
        old_path: None,
        new_path: Some(PathBuf::from(path)),
        language: Language::Erlang,
        binary: false,
        hunks: vec![Hunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: added_lines.len(),
            lines: added_lines
                .iter()
                .map(|s| HunkLine::Added((*s).to_owned()))
                .collect(),
        }],
    }
}

#[test]
fn fully_added_erl_yields_content_and_path() {
    let file = fully_added(
        "deps/rabbit/test/x_SUITE.erl",
        &["-module(x_SUITE).", "f() -> ok."],
    );
    let (path, content) = synthesise_added_file_content(&file).unwrap();
    assert_eq!(path.as_str(), "deps/rabbit/test/x_SUITE.erl");
    assert_eq!(content, "-module(x_SUITE).\nf() -> ok.\n");
}

#[test]
fn modified_file_is_not_synthesised() {
    let mut file = fully_added("deps/rabbit/src/x.erl", &["-module(x)."]);
    file.old_path = Some(PathBuf::from("deps/rabbit/src/x.erl"));
    assert!(synthesise_added_file_content(&file).is_none());
}

#[test]
fn binary_file_is_not_synthesised() {
    let mut file = fully_added("deps/rabbit/priv/blob.bin", &["data"]);
    file.binary = true;
    file.new_path = Some(PathBuf::from("deps/rabbit/priv/blob.bin"));
    assert!(synthesise_added_file_content(&file).is_none());
}

#[test]
fn non_erl_non_hrl_extension_is_not_synthesised() {
    let file = fully_added("deps/rabbit/test/x.md", &["# hi"]);
    assert!(synthesise_added_file_content(&file).is_none());
}

#[test]
fn capital_case_extension_is_accepted() {
    let file = fully_added("deps/rabbit/include/x.HRL", &["-define(X, 1)."]);
    let (path, content) = synthesise_added_file_content(&file).unwrap();
    assert_eq!(path.as_str(), "deps/rabbit/include/x.HRL");
    assert!(content.contains("-define(X, 1)."));
}

#[test]
fn added_lines_only_returns_none_when_hunk_carries_no_addition() {
    let file = PatchedFile {
        old_path: None,
        new_path: Some(PathBuf::from("deps/rabbit/src/x.erl")),
        language: Language::Erlang,
        binary: false,
        hunks: vec![Hunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 0,
            lines: vec![HunkLine::Context("unchanged".to_owned())],
        }],
    };
    assert!(synthesise_added_file_content(&file).is_none());
}

fn project(name: &str, family: ProjectFamily) -> Project {
    Project {
        name: ProjectName::new(name).unwrap(),
        git_url: Some(PathBuf::from("/tmp/p.git")),
        kind: ProjectKind::External,
        family,
        language: ConfigLanguage::Erlang,
        tag_prefix: "v".into(),
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

fn cfg_with(projects: Vec<Project>) -> Config {
    Config {
        projects,
        ..Config::default()
    }
}

#[test]
fn search_path_globs_empty_for_generic_only_workspace() {
    let cfg = cfg_with(vec![project("seshat", ProjectFamily::Generic)]);
    assert!(collect_search_path_globs(&cfg).is_empty());
}

#[test]
fn search_path_globs_populated_when_rabbitmq_family_present() {
    let cfg = cfg_with(vec![project("rabbit", ProjectFamily::Rabbitmq)]);
    let globs = collect_search_path_globs(&cfg);
    assert!(globs.iter().any(|g| g == "deps/*/test"));
    assert!(globs.iter().any(|g| g == "deps/*/src"));
    assert!(globs.iter().any(|g| g == "deps/rabbitmq_ct_helpers/src"));
}

#[test]
fn search_path_globs_deduplicate_across_projects() {
    let cfg = cfg_with(vec![
        project("a", ProjectFamily::Rabbitmq),
        project("b", ProjectFamily::Rabbitmq),
    ]);
    let globs = collect_search_path_globs(&cfg);
    let count_test = globs.iter().filter(|g| g.as_str() == "deps/*/test").count();
    assert_eq!(count_test, 1);
}
