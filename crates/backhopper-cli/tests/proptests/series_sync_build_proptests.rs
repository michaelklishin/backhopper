// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Properties of `build_sync_output`:
//!
//!  * every dep_NAME line ends up in exactly one of `pins`, `dropped_unconfigured`,
//!    or `skipped`
//!  * names starting with a digit always land in `skipped`
//!  * configured projects with valid names always land in `pins`

use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::config::{Language, Project, ProjectFamily, ProjectLayout};
use backhopper_core::model::names::{ProjectName, SeriesName};

use backhopper_cli::commands::series::build_sync_output;

fn project(name: &str) -> Project {
    Project {
        name: ProjectName::new(name).unwrap(),
        git_url: Some(PathBuf::from("/tmp/x.git")),
        kind: backhopper_core::config::ProjectKind::External,
        family: ProjectFamily::Generic,
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
        exclude_tag_markers: Vec::new(),
    }
}

fn valid_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,12}".prop_filter("not a `_repo`-suffixed alias", |s| {
        !s.ends_with("_commit") && !s.ends_with("_branch") && !s.ends_with("_repo")
    })
}

fn digit_first_name() -> impl Strategy<Value = String> {
    "[0-9][a-z0-9_]{0,12}".prop_filter("not a `_repo`-suffixed alias", |s| {
        !s.ends_with("_commit") && !s.ends_with("_branch") && !s.ends_with("_repo")
    })
}

fn version() -> impl Strategy<Value = String> {
    "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}".prop_map(String::from)
}

fn series_name() -> SeriesName {
    SeriesName::new("rabbitmq-x").unwrap()
}

proptest! {
    #[test]
    fn every_dep_line_lands_in_exactly_one_bucket(
        configured in prop::collection::vec(valid_name(), 0..4),
        unconfigured in prop::collection::vec(valid_name(), 0..4),
        invalid in prop::collection::vec(digit_first_name(), 0..4),
        ver in version(),
    ) {
        let mut lines = Vec::new();
        for n in configured.iter().chain(unconfigured.iter()).chain(invalid.iter()) {
            lines.push(format!("dep_{n} = hex {ver}\n"));
        }
        let mk: String = lines.concat();
        let projects: Vec<_> = configured.iter().map(|n| project(n)).collect();
        let out = build_sync_output(&mk, &series_name(), &projects);

        let total_lines = configured.len() + unconfigured.len() + invalid.len();
        let bucketed = out.pins.len() + out.dropped_unconfigured.len() + out.skipped.len();
        prop_assert!(
            bucketed <= total_lines,
            "buckets must not exceed input lines: {bucketed} > {total_lines}"
        );
        for n in &invalid {
            prop_assert!(
                out.skipped.iter().any(|s| s.name == *n),
                "digit-first name {n} must be skipped: {out:?}",
            );
        }
        for n in &configured {
            let in_pins = out.pins.iter().any(|p| p.project == *n);
            let in_skipped = out.skipped.iter().any(|s| s.name == *n);
            prop_assert!(
                in_pins || in_skipped,
                "configured name {n} must land in pins or skipped: {out:?}",
            );
        }
    }

    #[test]
    fn pins_are_sorted_alphabetically(
        names in prop::collection::vec(valid_name(), 0..6),
        ver in version(),
    ) {
        let mut lines = Vec::new();
        for n in &names {
            lines.push(format!("dep_{n} = hex {ver}\n"));
        }
        let mk: String = lines.concat();
        let projects: Vec<_> = names.iter().map(|n| project(n)).collect();
        let out = build_sync_output(&mk, &series_name(), &projects);
        let mut sorted = out.pins.clone();
        sorted.sort_by(|a, b| a.project.cmp(&b.project));
        prop_assert_eq!(out.pins, sorted);
    }

    #[test]
    fn skipped_is_sorted_alphabetically(
        names in prop::collection::vec(digit_first_name(), 0..6),
        ver in version(),
    ) {
        let mut lines = Vec::new();
        for n in &names {
            lines.push(format!("dep_{n} = hex {ver}\n"));
        }
        let mk: String = lines.concat();
        let out = build_sync_output(&mk, &series_name(), &[]);
        let mut sorted = out.skipped.clone();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        prop_assert_eq!(out.skipped, sorted);
    }
}
