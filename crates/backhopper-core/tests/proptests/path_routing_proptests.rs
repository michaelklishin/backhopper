// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Project::owns_path` and `classify_paths_for_pin` invariants.

use std::path::{Path, PathBuf};

use proptest::prelude::*;

use backhopper_core::compat::routing::{classify_paths_for_pin, project_owns_path};
use backhopper_core::config::{Project, ProjectFamily, ProjectKind, ProjectLayout};
use backhopper_core::model::names::ProjectName;

fn project(name: &str, scan_paths: Vec<String>, app_roots: Vec<String>) -> Project {
    Project {
        name: ProjectName::new(name).unwrap(),
        git_url: Some(PathBuf::from("/tmp/x.git")),
        kind: ProjectKind::External,
        family: ProjectFamily::Generic,
        language: backhopper_core::config::Language::Erlang,
        tag_prefix: String::new(),
        public_modules: Vec::new(),
        internal_modules: Vec::new(),
        layout: ProjectLayout::SingleApp,
        scan_paths,
        app_roots,
        include_apps: Vec::new(),
        exclude_apps: Vec::new(),
        excluded_subdirs: Vec::new(),
        tag_pattern: None,
        min_tag: None,
        exclude_tag_markers: Vec::new(),
    }
}

fn safe_path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(String::from)
}

fn safe_path(depth: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(safe_path_segment(), 1..=depth).prop_map(|segs| segs.join("/"))
}

proptest! {
    // Paths inside a configured scan_path prefix always classify as owned.
    #[test]
    fn paths_under_scan_path_prefix_are_owned(
        root in safe_path_segment(),
        tail in safe_path(4),
    ) {
        let p = project("demo", vec![root.clone()], vec![]);
        let path_str = format!("{root}/{tail}");
        prop_assert!(p.owns_path(Path::new(&path_str)));
    }

    // Glob suffix on a scan_path does not change the prefix-match outcome.
    #[test]
    fn glob_suffix_preserves_prefix_match(
        root in safe_path_segment(),
        tail in safe_path(3),
    ) {
        let bare = project("demo", vec![root.clone()], vec![]);
        let globbed = project("demo", vec![format!("{root}/**/*.erl")], vec![]);
        let path_str = format!("{root}/{tail}");
        prop_assert_eq!(
            bare.owns_path(Path::new(&path_str)),
            globbed.owns_path(Path::new(&path_str))
        );
    }

    // A path that does not share any leading segment with any scan_path
    // is never owned.
    #[test]
    fn unrelated_path_is_not_owned(
        scan in safe_path_segment(),
        other in safe_path_segment(),
    ) {
        prop_assume!(scan != other);
        let p = project("demo", vec![scan], vec![]);
        let path_str = format!("{other}/ra_server.erl");
        prop_assert!(!p.owns_path(Path::new(&path_str)));
    }

    // Classification: when every touched path is owned by the pin's
    // project, the routing reports any_in_scope true with no out-of-scope
    // owners and zero untracked.
    #[test]
    fn all_in_scope_means_no_attribution(
        scan in safe_path_segment(),
        paths in prop::collection::vec(safe_path(3), 1..6),
    ) {
        let demo = project("demo", vec![scan.clone()], vec![]);
        let touched: Vec<PathBuf> = paths
            .into_iter()
            .map(|t| PathBuf::from(format!("{scan}/{t}.erl")))
            .collect();
        let siblings = vec![&demo];
        let routing = classify_paths_for_pin(&touched, &demo, &siblings);
        prop_assert!(routing.any_in_scope);
        prop_assert!(routing.out_of_scope_owners.is_empty());
        prop_assert_eq!(routing.untracked_count, 0);
    }

    // Empty scope (no scan_paths, no app_roots) falls back to "owns
    // everything" for non-self projects.
    #[test]
    fn empty_scope_owns_every_path(path in safe_path(5)) {
        let p = project("demo", vec![], vec![]);
        prop_assert!(project_owns_path(&p, Path::new(&path)));
    }
}
