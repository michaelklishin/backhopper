// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Focused unit tests for `check_snapshot_dir_escape`. Exercises the
//! pure function with synthetic configs so we do not need a tempdir or
//! a real snapshot store.

use std::path::PathBuf;

use backhopper_core::config::{Config, Defaults};

use backhopper_cli::CliError;
use backhopper_cli::cli::{Formatter, GlobalArgs};
use backhopper_cli::commands::context::check_snapshot_dir_escape;

fn defaults_with(dir: &str) -> Defaults {
    Defaults {
        snapshot_dir: PathBuf::from(dir),
        fallback_branch: "main".into(),
        scan_paths: vec![],
    }
}

fn fake_global_args() -> GlobalArgs {
    GlobalArgs {
        config_file_path: None,
        snapshot_dir_path: None,
        formatter: Formatter::Text,
        quiet: false,
        verbose: 0,
        non_interactive: false,
        table_style: bel7_cli::TableStyle::Modern,
    }
}

#[test]
fn absolute_dir_without_parent_dir_components_is_accepted() {
    let cfg = Config {
        config_path: PathBuf::from("/etc/backhopper/backhopper.toml"),
        defaults: defaults_with("/var/lib/backhopper/snapshots"),
        ..Default::default()
    };
    let result = check_snapshot_dir_escape(
        &fake_global_args(),
        &cfg,
        &PathBuf::from("/var/lib/backhopper/snapshots"),
    );
    assert!(result.is_ok());
}

#[test]
fn relative_dir_without_parent_dir_is_accepted() {
    let cfg = Config {
        config_path: PathBuf::from("/etc/backhopper/backhopper.toml"),
        defaults: defaults_with("snapshots"),
        ..Default::default()
    };
    let result = check_snapshot_dir_escape(
        &fake_global_args(),
        &cfg,
        &PathBuf::from("/etc/backhopper/snapshots"),
    );
    assert!(result.is_ok());
}

#[test]
fn parent_dir_components_produce_snapshot_dir_escape() {
    let cfg = Config {
        config_path: PathBuf::from("/etc/backhopper/backhopper.toml"),
        defaults: defaults_with("../escapes"),
        ..Default::default()
    };
    let result = check_snapshot_dir_escape(
        &fake_global_args(),
        &cfg,
        &PathBuf::from("/etc/backhopper/../escapes"),
    );
    match result {
        Err(CliError::SnapshotDirEscape {
            configured,
            resolved,
            root,
        }) => {
            assert_eq!(configured, PathBuf::from("../escapes"));
            assert_eq!(resolved, PathBuf::from("/etc/backhopper/../escapes"));
            assert_eq!(root, PathBuf::from("/etc/backhopper"));
        }
        other => panic!("expected SnapshotDirEscape, got {other:?}"),
    }
}

#[test]
fn cli_override_changes_the_reported_root_to_cwd() {
    let cfg = Config {
        config_path: PathBuf::from("/etc/backhopper/backhopper.toml"),
        defaults: defaults_with("snapshots"),
        ..Default::default()
    };
    let mut args = fake_global_args();
    args.snapshot_dir_path = Some(PathBuf::from("../from-cli"));
    let result = check_snapshot_dir_escape(&args, &cfg, &PathBuf::from("/cwd/../from-cli"));
    match result {
        Err(CliError::SnapshotDirEscape { configured, .. }) => {
            assert_eq!(configured, PathBuf::from("../from-cli"));
        }
        other => panic!("expected SnapshotDirEscape, got {other:?}"),
    }
}
