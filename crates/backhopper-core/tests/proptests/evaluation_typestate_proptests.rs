// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Property tests for the `EvaluationContext<S>` pipeline: any sequence
//! of `for_pin → with_scope → with_files` must preserve `pin`, `snapshot`,
//! `scope`, and (for `Sourced`) `files`. State-carrying inner data makes
//! these invariants type-true; the proptests guard against regressions
//! that re-introduce optional fields.

use std::path::PathBuf;

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, EvaluationInput};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader, state};

fn make_snap(project: &str) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(
        SnapshotHeader {
            project: ProjectName::new(project).unwrap(),
            tag: TagName::new("v1.0.0").unwrap(),
            branch: None,
            commit: CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: vec!["src/**/*.erl".into()],
            apps_scanned: Vec::new(),
            generated_by: "test".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
            extractor_version: String::new(),
        },
        vec![],
        vec![],
    )
    .into_canonical()
}

fn make_pin(project: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

proptest! {
    #[test]
    fn pinned_to_scoped_preserves_pin_and_snapshot(project in "[a-z][a-z0-9_]{0,8}") {
        let snap = make_snap(&project);
        let scope = PinScope::from_snapshot(ProjectName::new(&project).unwrap(), &snap, []);
        let scoped = EvaluationContext::for_pin(make_pin(&project), snap)
            .with_scope(scope);
        prop_assert_eq!(scoped.pin().project.as_str(), project.as_str());
        prop_assert!(scoped.files().is_none());
    }

    #[test]
    fn scoped_to_sourced_preserves_pin_scope_and_attaches_files(
        project in "[a-z][a-z0-9_]{0,8}",
        path_str in "[a-z]{1,10}\\.erl",
    ) {
        let snap = make_snap(&project);
        let scope = PinScope::from_snapshot(ProjectName::new(&project).unwrap(), &snap, []);
        let files = EvaluationFiles::new()
            .with(PathBuf::from(path_str), Some(b"-module(x).".to_vec()));
        let sourced = EvaluationContext::new(make_pin(&project), snap, scope)
            .with_files(files);
        prop_assert_eq!(sourced.pin().project.as_str(), project.as_str());
        prop_assert!(sourced.files().is_some(), "Sourced must always carry files");
        prop_assert_eq!(sourced.files().unwrap().len(), 1);
    }

    #[test]
    fn new_shortcut_matches_explicit_two_step(project in "[a-z][a-z0-9_]{0,8}") {
        let snap_a = make_snap(&project);
        let scope_a = PinScope::from_snapshot(ProjectName::new(&project).unwrap(), &snap_a, []);
        let snap_b = make_snap(&project);
        let scope_b = PinScope::from_snapshot(ProjectName::new(&project).unwrap(), &snap_b, []);
        let shortcut = EvaluationContext::new(make_pin(&project), snap_a, scope_a);
        let staged = EvaluationContext::for_pin(make_pin(&project), snap_b).with_scope(scope_b);
        prop_assert_eq!(shortcut, staged);
    }
}
