// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Language, Project, ProjectLayout};
use backhopper_core::model::names::{ProjectName, TagName};

use backhopper_cli::commands::snapshots::filter_tags_for_project;
use proptest::prelude::*;

fn project_with_markers(markers: Vec<String>) -> Project {
    Project {
        name: ProjectName::new("p").unwrap(),
        git_url: PathBuf::from("/tmp/p.git"),
        language: Language::Erlang,
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
        exclude_tag_markers: markers,
    }
}

fn safe_tag() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9.-]{0,24}".prop_filter("non-empty", |s: &String| !s.is_empty())
}

proptest! {
    #[test]
    fn filtered_tags_never_contain_any_marker(
        markers in prop::collection::vec("[-][a-z]{2,8}", 1..4),
        raw in prop::collection::vec(safe_tag(), 0..30),
    ) {
        let project = project_with_markers(markers.clone());
        let tags: Vec<TagName> = raw
            .iter()
            .filter_map(|s| TagName::new(s.clone()).ok())
            .collect();
        let filtered = filter_tags_for_project(tags, &project, None);
        for tag in &filtered {
            for m in &markers {
                prop_assert!(
                    !tag.as_str().contains(m.as_str()),
                    "filtered tag {} still contains marker {}",
                    tag.as_str(),
                    m,
                );
            }
        }
    }

    #[test]
    fn empty_marker_list_is_identity_for_unrelated_tags(
        raw in prop::collection::vec(safe_tag(), 0..20),
    ) {
        let project = project_with_markers(Vec::new());
        let tags: Vec<TagName> = raw
            .iter()
            .filter_map(|s| TagName::new(s.clone()).ok())
            .collect();
        let before = tags.clone();
        let after = filter_tags_for_project(tags, &project, None);
        prop_assert_eq!(after, before);
    }
}
