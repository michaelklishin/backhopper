// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_git::GitError;
use std::path::PathBuf;

use bel7_cli::{ExitCode, ExitCodeProvider};

use backhopper_cli::CliError;

#[test]
fn config_not_found_maps_to_no_input() {
    let e = CliError::ConfigNotFound {
        tried: vec![PathBuf::from("/x")],
    };
    assert_eq!(
        <CliError as ExitCodeProvider>::exit_code(&e),
        ExitCode::NoInput
    );
    assert_eq!(e.exit_code(), ExitCode::NoInput);
}

#[test]
fn config_not_found_lists_every_tried_path_in_message() {
    let e = CliError::ConfigNotFound {
        tried: vec![
            PathBuf::from("./backhopper.toml"),
            PathBuf::from("/home/me/.config/backhopper/backhopper.toml"),
        ],
    };
    let msg = e.to_string();
    assert!(msg.contains("./backhopper.toml"), "msg was: {msg}");
    assert!(
        msg.contains("/home/me/.config/backhopper/backhopper.toml"),
        "msg was: {msg}"
    );
}

#[test]
fn invalid_input_maps_to_usage() {
    let e = CliError::InvalidInput("bad".into());
    assert_eq!(e.exit_code(), ExitCode::Usage);
}

#[test]
fn io_error_maps_to_io_err() {
    let e = CliError::Io(std::io::Error::other("boom"));
    assert_eq!(e.exit_code(), ExitCode::IoErr);
}

#[test]
fn snapshot_dir_escape_maps_to_usage() {
    let e = CliError::SnapshotDirEscape {
        configured: PathBuf::from("../foo"),
        resolved: PathBuf::from("/home/u/cfg/../foo"),
        root: PathBuf::from("/home/u/cfg"),
    };
    assert_eq!(e.exit_code(), ExitCode::Usage);
    let hint = e.hint().unwrap();
    assert!(hint.contains("absolute path"));
}

#[test]
fn config_not_found_hint_points_at_init() {
    let e = CliError::ConfigNotFound {
        tried: vec![PathBuf::from("./backhopper.toml")],
    };
    let hint = e.hint().unwrap();
    assert!(hint.contains("backhopper init"), "hint was: {hint}");
}

#[test]
fn invalid_input_has_no_hint_by_default() {
    let e = CliError::InvalidInput("nope".into());
    assert!(e.hint().is_none());
    assert!(e.detail().is_none());
}

#[test]
fn git_hint_for_commit_not_found_suggests_git_fetch() {
    let e = CliError::Git(GitError::CommitNotFound("deadbeef".into()));
    let hint = e.hint().unwrap();
    assert!(hint.contains("git fetch"));
}

#[test]
fn git_hint_for_tag_not_found_suggests_fetch_tags() {
    let e = CliError::Git(GitError::TagNotFound("v1.0.0".into()));
    let hint = e.hint().unwrap();
    assert!(hint.contains("--tags"));
}

#[test]
fn git_hint_for_open_failed_mentions_git_url() {
    let e = CliError::Git(GitError::OpenFailed("nope".into()));
    let hint = e.hint().unwrap();
    assert!(hint.contains("git_url"));
}

#[test]
fn git_hint_for_io_error_has_no_canned_hint() {
    let e = CliError::Git(GitError::Io(std::io::Error::other("x")));
    assert!(e.hint().is_none());
}

#[test]
fn missing_snapshots_detail_lists_pins() {
    let e = CliError::MissingSnapshots {
        missing: vec![Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.0.0").unwrap(),
        )],
        remediation: "run: ...".into(),
    };
    let detail = e.detail().unwrap();
    assert!(detail.contains("ra @ v2.0.0"));
}
