// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::{ArgAction, CommandFactory};

use backhopper_cli::Cli;

#[test]
fn cli_command_metadata_is_well_formed() {
    let mut cmd = Cli::command();
    cmd.build();
    assert_eq!(cmd.get_name(), "backhopper");
    let names: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
    for expected in ["projects", "snapshots", "check", "shell"] {
        assert!(names.contains(&expected), "missing subcommand: {expected}");
    }
}

#[test]
fn completions_subcommand_accepts_shell_as_value_enum() {
    let mut cmd = Cli::command();
    cmd.build();
    let shell = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "shell")
        .expect("shell subcommand missing");
    let completions = shell
        .get_subcommands()
        .find(|s| s.get_name() == "completions")
        .expect("completions subcommand missing under shell");
    let shell_arg = completions
        .get_arguments()
        .find(|a| a.get_id().as_str() == "shell")
        .expect("shell positional argument missing on completions");
    let values: Vec<String> = shell_arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_owned())
        .collect();
    for expected in ["bash", "zsh", "fish", "nushell", "powershell"] {
        assert!(
            values.iter().any(|v| v == expected),
            "shell value `{expected}` missing from {values:?}"
        );
    }
}

#[test]
fn table_style_arg_accepts_bel7_value_enum() {
    let mut cmd = Cli::command();
    cmd.build();
    let arg = cmd
        .get_arguments()
        .find(|a| a.get_id().as_str() == "table_style")
        .expect("table_style global flag missing");
    let values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_owned())
        .collect();
    for expected in ["modern", "markdown", "ascii", "psql"] {
        assert!(
            values.iter().any(|v| v == expected),
            "table style `{expected}` missing from {values:?}"
        );
    }
}

#[test]
fn projects_group_has_list_and_show() {
    let mut cmd = Cli::command();
    cmd.build();
    let projects = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "projects")
        .unwrap();
    let names: Vec<_> = projects.get_subcommands().map(|s| s.get_name()).collect();
    assert!(names.contains(&"list"));
    assert!(names.contains(&"show"));
}

#[test]
fn check_group_has_patch_commit_range() {
    let mut cmd = Cli::command();
    cmd.build();
    let group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "check")
        .unwrap();
    let names: Vec<_> = group.get_subcommands().map(|s| s.get_name()).collect();
    assert!(names.contains(&"patch"));
    assert!(names.contains(&"commit"));
    assert!(names.contains(&"range"));
}

#[test]
fn check_patch_advertises_diagnostic_flags() {
    let mut cmd = Cli::command();
    cmd.build();
    let group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "check")
        .unwrap();
    let patch = group
        .get_subcommands()
        .find(|s| s.get_name() == "patch")
        .unwrap();
    let arg_names: Vec<&str> = patch.get_arguments().map(|a| a.get_id().as_str()).collect();
    assert!(arg_names.contains(&"show_untracked_calls"));
    assert!(arg_names.contains(&"show_otp_calls"));
}

#[test]
fn verbose_flag_is_counted() {
    let mut cmd = Cli::command();
    cmd.build();
    let verbose = cmd
        .get_arguments()
        .find(|a| a.get_id().as_str() == "verbose")
        .expect("verbose flag missing");
    assert!(
        matches!(verbose.get_action(), ArgAction::Count),
        "verbose should accept repeats for graduated verbosity (was {:?})",
        verbose.get_action()
    );
}
