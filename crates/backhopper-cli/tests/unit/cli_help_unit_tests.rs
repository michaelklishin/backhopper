use clap::{ArgAction, CommandFactory};

use backhopper_cli::Cli;

#[test]
fn cli_command_metadata_is_well_formed() {
    let mut cmd = Cli::command();
    cmd.build();
    assert_eq!(cmd.get_name(), "backhopper");
    assert!(cmd.get_subcommands().any(|s| s.get_name() == "projects"));
    assert!(cmd.get_subcommands().any(|s| s.get_name() == "snapshots"));
    assert!(
        cmd.get_subcommands()
            .any(|s| s.get_name() == "compatibility")
    );
    assert!(cmd.get_subcommands().any(|s| s.get_name() == "completions"));
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
fn compatibility_group_has_patch_commit_range() {
    let mut cmd = Cli::command();
    cmd.build();
    let group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "compatibility")
        .unwrap();
    let names: Vec<_> = group.get_subcommands().map(|s| s.get_name()).collect();
    assert!(names.contains(&"patch"));
    assert!(names.contains(&"commit"));
    assert!(names.contains(&"range"));
}

#[test]
fn compatibility_patch_advertises_diagnostic_flags() {
    let mut cmd = Cli::command();
    cmd.build();
    let group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "compatibility")
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
