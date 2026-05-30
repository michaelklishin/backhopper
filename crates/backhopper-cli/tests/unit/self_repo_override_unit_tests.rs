// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use backhopper_core::model::names::{GitRef, ProjectName};
use backhopper_core::model::pin::PinSpec;

use backhopper_cli::commands::self_snapshot::effective_self_repo;

fn self_pin(override_path: Option<PathBuf>) -> PinSpec {
    PinSpec::SelfRef {
        project: ProjectName::new("host").unwrap(),
        git_ref: GitRef::new("v4.1.x").unwrap(),
        repo_dir_path: override_path,
    }
}

#[test]
fn override_wins_over_cli_fallback() {
    let spec = self_pin(Some(PathBuf::from("/tmp/override.git")));
    let fallback = PathBuf::from("/tmp/fallback.git");
    let resolved = effective_self_repo(&spec, Some(&fallback)).unwrap();
    assert_eq!(resolved, Path::new("/tmp/override.git"));
}

#[test]
fn fallback_used_when_override_absent() {
    let spec = self_pin(None);
    let fallback = PathBuf::from("/tmp/fallback.git");
    let resolved = effective_self_repo(&spec, Some(&fallback)).unwrap();
    assert_eq!(resolved, Path::new("/tmp/fallback.git"));
}

#[test]
fn both_unset_yields_invalid_input_error() {
    let spec = self_pin(None);
    let err = effective_self_repo(&spec, None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("repo_dir_path"), "got: {msg}");
    assert!(msg.contains("--repo-dir-path"), "got: {msg}");
}

#[test]
fn override_unused_when_set_even_if_fallback_also_set() {
    let spec = self_pin(Some(PathBuf::from("/tmp/override.git")));
    let resolved = effective_self_repo(&spec, None).unwrap();
    assert_eq!(resolved, Path::new("/tmp/override.git"));
}
