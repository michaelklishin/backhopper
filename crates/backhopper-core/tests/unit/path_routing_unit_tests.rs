// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-pin path routing: which configured project owns a touched path,
//! and what the resulting `PathRouting` summary says about the verdict.

use std::path::{Path, PathBuf};

use backhopper_core::compat::routing::{PathRouting, classify_paths_for_pin, project_owns_path};
use backhopper_core::config::{Project, ProjectFamily, ProjectKind, ProjectLayout};
use backhopper_core::model::names::ProjectName;

fn project(name: &str, is_self: bool, scan_paths: &[&str], app_roots: &[&str]) -> Project {
    Project {
        name: ProjectName::new(name).unwrap(),
        git_url: if is_self {
            None
        } else {
            Some(PathBuf::from("/tmp/x.git"))
        },
        kind: if is_self {
            ProjectKind::SelfRepo
        } else {
            ProjectKind::External
        },
        family: ProjectFamily::Generic,
        language: backhopper_core::config::Language::Erlang,
        tag_prefix: String::new(),
        public_modules: Vec::new(),
        internal_modules: Vec::new(),
        layout: ProjectLayout::SingleApp,
        scan_paths: scan_paths.iter().map(|s| (*s).to_owned()).collect(),
        app_roots: app_roots.iter().map(|s| (*s).to_owned()).collect(),
        include_apps: Vec::new(),
        exclude_apps: Vec::new(),
        excluded_subdirs: Vec::new(),
        tag_pattern: None,
        min_tag: None,
        exclude_tag_markers: Vec::new(),
    }
}

#[test]
fn owns_path_matches_literal_scan_path_prefix() {
    let p = project("demo", false, &["src"], &[]);
    assert!(project_owns_path(&p, Path::new("src/foo.erl")));
    assert!(project_owns_path(&p, Path::new("src/sub/bar.erl")));
    assert!(!project_owns_path(&p, Path::new("test/baz.erl")));
}

#[test]
fn owns_path_strips_glob_suffix_to_directory_prefix() {
    let p = project("demo", false, &["src/**/*.erl"], &[]);
    assert!(project_owns_path(&p, Path::new("src/foo.erl")));
    assert!(project_owns_path(&p, Path::new("src/deep/nested/bar.erl")));
    assert!(!project_owns_path(&p, Path::new("lib/foo.erl")));
}

#[test]
fn owns_path_honors_app_roots() {
    let p = project("otp", false, &[], &["lib/kernel", "lib/stdlib"]);
    assert!(project_owns_path(&p, Path::new("lib/kernel/src/code.erl")));
    assert!(project_owns_path(&p, Path::new("lib/stdlib/src/lists.erl")));
    assert!(!project_owns_path(
        &p,
        Path::new("lib/other_app/src/foo.erl")
    ));
}

#[test]
fn owns_path_falls_back_to_deps_overlay_for_external_project() {
    let p = project("ra", false, &[], &[]);
    assert!(project_owns_path(&p, Path::new("deps/ra/src/foo.erl")));
}

#[test]
fn owns_path_skips_deps_overlay_for_self_project() {
    let p = project("rabbit", true, &["src"], &[]);
    assert!(!project_owns_path(&p, Path::new("deps/rabbit/src/foo.erl")));
}

#[test]
fn owns_path_owns_everything_when_no_scope_configured() {
    let p = project("demo", false, &[], &[]);
    assert!(project_owns_path(&p, Path::new("arbitrary/path/x.erl")));
    assert!(project_owns_path(&p, Path::new("src/y.erl")));
}

#[test]
fn classify_paths_for_pin_records_in_scope_when_pin_owns_every_path() {
    let demo = project("demo", false, &["src"], &[]);
    let siblings = vec![&demo];
    let routing = classify_paths_for_pin(
        &[PathBuf::from("src/foo.erl"), PathBuf::from("src/bar.erl")],
        &demo,
        &siblings,
    );
    assert!(routing.any_in_scope);
    assert!(routing.out_of_scope_owners.is_empty());
    assert_eq!(routing.untracked_count, 0);
}

#[test]
fn classify_paths_attributes_out_of_scope_to_sibling_owner() {
    let cuttlefish = project("cuttlefish", true, &["src"], &[]);
    let otp = project("otp", false, &[], &["lib/kernel", "lib/stdlib"]);
    let siblings = vec![&cuttlefish, &otp];
    let routing = classify_paths_for_pin(
        &[PathBuf::from("src/cuttlefish_datatypes.erl")],
        &otp,
        &siblings,
    );
    assert!(!routing.any_in_scope);
    assert_eq!(
        routing.first_out_of_scope_owner().map(|p| p.as_str()),
        Some("cuttlefish")
    );
    assert_eq!(routing.untracked_count, 0);
}

#[test]
fn classify_paths_marks_unclaimed_path_as_untracked() {
    let demo = project("demo", false, &["src"], &[]);
    let siblings = vec![&demo];
    let routing =
        classify_paths_for_pin(&[PathBuf::from("vendored/upstream.erl")], &demo, &siblings);
    assert!(!routing.any_in_scope);
    assert!(routing.out_of_scope_owners.is_empty());
    assert_eq!(routing.untracked_count, 1);
}

#[test]
fn classify_paths_mixed_keeps_in_scope_signal() {
    let cuttlefish = project("cuttlefish", true, &["src"], &[]);
    let otp = project("otp", false, &[], &["lib/kernel"]);
    let siblings = vec![&cuttlefish, &otp];
    let routing = classify_paths_for_pin(
        &[
            PathBuf::from("src/cuttlefish_datatypes.erl"),
            PathBuf::from("lib/kernel/src/code.erl"),
        ],
        &otp,
        &siblings,
    );
    assert!(routing.any_in_scope);
    assert_eq!(
        routing.first_out_of_scope_owner().map(|p| p.as_str()),
        Some("cuttlefish")
    );
}

#[test]
fn classify_paths_returns_first_owner_alphabetically_when_multiple() {
    let alpha = project("alpha_lib", false, &["apps/shared"], &[]);
    let bravo = project("bravo_lib", false, &["apps/shared"], &[]);
    let target = project("target", false, &["src"], &[]);
    let siblings = vec![&alpha, &bravo, &target];
    let routing =
        classify_paths_for_pin(&[PathBuf::from("apps/shared/foo.erl")], &target, &siblings);
    assert_eq!(
        routing.first_out_of_scope_owner().map(|p| p.as_str()),
        Some("alpha_lib")
    );
}

#[test]
fn empty_routing_has_no_attribution() {
    let routing = PathRouting::default();
    assert!(!routing.has_any_attribution());
}

#[test]
fn empty_touched_yields_no_attribution_and_no_in_scope() {
    let demo = project("demo", false, &["src"], &[]);
    let siblings = vec![&demo];
    let routing = classify_paths_for_pin(&[], &demo, &siblings);
    assert!(!routing.any_in_scope);
    assert!(routing.out_of_scope_owners.is_empty());
    assert_eq!(routing.untracked_count, 0);
    assert!(!routing.has_any_attribution());
}

#[test]
fn deps_overlay_owns_path_for_external_project_with_scope() {
    let p = project("ra", false, &["src"], &[]);
    assert!(project_owns_path(&p, Path::new("deps/ra/src/foo.erl")));
    assert!(project_owns_path(&p, Path::new("src/foo.erl")));
    assert!(!project_owns_path(&p, Path::new("apps/other/foo.erl")));
}

#[test]
fn owns_path_handles_trailing_slash_in_scan_path() {
    let p = project("demo", false, &["src/"], &[]);
    assert!(project_owns_path(&p, Path::new("src/foo.erl")));
    assert!(project_owns_path(&p, Path::new("src/sub/bar.erl")));
}

#[test]
fn owns_path_rejects_partial_segment_match() {
    let p = project("demo", false, &["src"], &[]);
    assert!(!project_owns_path(&p, Path::new("source/foo.erl")));
    assert!(!project_owns_path(&p, Path::new("srcs/foo.erl")));
}
