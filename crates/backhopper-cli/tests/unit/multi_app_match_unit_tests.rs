// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Language, Project, ProjectLayout};
use backhopper_core::model::names::{ApplicationName, ProjectName};

use backhopper_cli::commands::snapshots::multi_app_match;

fn otp_project() -> Project {
    Project {
        name: ProjectName::new("otp").unwrap(),
        git_url: PathBuf::from("/tmp/otp.git"),
        language: Language::Erlang,
        tag_prefix: "v".into(),
        public_modules: Vec::new(),
        internal_modules: Vec::new(),
        layout: ProjectLayout::ErlangOtp,
        scan_paths: Vec::new(),
        app_roots: vec!["lib/*".into(), "erts/preloaded".into()],
        include_apps: Vec::new(),
        exclude_apps: vec![
            ApplicationName::new("wx").unwrap(),
            ApplicationName::new("megaco").unwrap(),
        ],
        excluded_subdirs: vec!["examples".into(), "test".into()],
        tag_pattern: None,
        min_tag: None,
    }
}

#[test]
fn wildcard_root_matches_app_src_path() {
    let p = otp_project();
    let app = multi_app_match("lib/stdlib/src/lists.erl", &p).unwrap();
    assert_eq!(app.as_str(), "stdlib");
}

#[test]
fn excluded_app_drops_its_files() {
    let p = otp_project();
    assert!(multi_app_match("lib/wx/src/wx.erl", &p).is_none());
    assert!(multi_app_match("lib/megaco/src/megaco.erl", &p).is_none());
}

#[test]
fn excluded_subdir_drops_test_and_examples() {
    let p = otp_project();
    assert!(multi_app_match("lib/stdlib/test/lists_SUITE.erl", &p).is_none());
    assert!(multi_app_match("lib/stdlib/examples/foo.erl", &p).is_none());
    // src is kept
    assert!(multi_app_match("lib/stdlib/src/lists.erl", &p).is_some());
}

#[test]
fn include_apps_intersects_when_non_empty() {
    let mut p = otp_project();
    p.include_apps = vec![ApplicationName::new("stdlib").unwrap()];
    assert!(multi_app_match("lib/stdlib/src/lists.erl", &p).is_some());
    assert!(multi_app_match("lib/kernel/src/file.erl", &p).is_none());
}

#[test]
fn literal_root_uses_trailing_segment_as_app_name() {
    let p = otp_project();
    let app = multi_app_match("erts/preloaded/src/erlang.erl", &p).unwrap();
    assert_eq!(app.as_str(), "preloaded");
}

#[test]
fn path_outside_any_root_is_dropped() {
    let p = otp_project();
    assert!(multi_app_match("docs/README.md", &p).is_none());
    assert!(multi_app_match("HOWTO/INSTALL.md", &p).is_none());
}

#[test]
fn non_erl_or_hrl_files_are_dropped_even_inside_an_app() {
    let p = otp_project();
    assert!(multi_app_match("lib/stdlib/src/Makefile", &p).is_none());
    assert!(multi_app_match("lib/stdlib/AUTHORS", &p).is_none());
}

#[test]
fn hrl_files_under_include_are_kept() {
    let p = otp_project();
    let app = multi_app_match("lib/kernel/include/file.hrl", &p).unwrap();
    assert_eq!(app.as_str(), "kernel");
}

#[test]
fn excluded_subdir_filter_treats_directory_boundary_correctly() {
    // path "testing.erl" should NOT be excluded as it's not in the test/ subdir
    let mut p = otp_project();
    p.excluded_subdirs = vec!["test".into()];
    assert!(multi_app_match("lib/stdlib/src/testing.erl", &p).is_some());
    assert!(multi_app_match("lib/stdlib/test/x.erl", &p).is_none());
}

#[test]
fn invalid_app_name_dir_is_skipped() {
    let p = otp_project();
    // dirs with uppercase or unexpected chars don't parse as ApplicationName,
    // so they're skipped entirely
    assert!(multi_app_match("lib/UPPER/src/x.erl", &p).is_none());
}

#[test]
fn literal_root_does_not_swallow_sibling_paths() {
    // `erts/preloaded` is a literal root; `erts/emulator/...` shares the
    // `erts/` prefix but is NOT under preloaded and must be rejected.
    let p = otp_project();
    assert!(multi_app_match("erts/emulator/beam/foo.erl", &p).is_none());
    assert!(multi_app_match("erts/preloaded/src/erlang.erl", &p).is_some());
}

#[test]
fn file_named_like_an_excluded_subdir_is_not_treated_as_one() {
    // A real source file whose stem happens to collide with an excluded
    // subdir name (e.g. `test.erl` directly under `src/`) must NOT be
    // filtered out: the filter targets directory boundaries, not filenames.
    let p = otp_project();
    assert!(multi_app_match("lib/stdlib/src/test.erl", &p).is_some());
    assert!(multi_app_match("lib/stdlib/src/examples.erl", &p).is_some());
}
