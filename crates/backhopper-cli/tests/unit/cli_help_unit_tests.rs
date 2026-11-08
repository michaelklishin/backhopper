// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::{ArgAction, CommandFactory, Parser};

use backhopper_cli::Cli;

#[test]
fn cli_command_metadata_is_well_formed() {
    let mut cmd = Cli::command();
    cmd.build();
    assert_eq!(cmd.get_name(), "backhopper");
    let names: Vec<&str> = cmd.get_subcommands().map(|c| c.get_name()).collect();
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
    let names: Vec<_> = projects.get_subcommands().map(|c| c.get_name()).collect();
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
    let names: Vec<_> = group.get_subcommands().map(|c| c.get_name()).collect();
    assert!(names.contains(&"patch"));
    assert!(names.contains(&"commit"));
    assert!(names.contains(&"range"));
}

// drift guard: catches a dropped verb and a documented verb the binary never shipped
#[test]
fn check_group_subcommand_set_is_exact() {
    let mut cmd = Cli::command();
    cmd.build();
    let group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "check")
        .unwrap();
    let mut names: Vec<&str> = group
        .get_subcommands()
        .map(|c| c.get_name())
        .filter(|n| *n != "help")
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "batch", "cascade", "commit", "merge", "patch", "pr", "range"
        ]
    );
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
    assert!(arg_names.contains(&"suggest_projects"));
    assert!(arg_names.contains(&"write_suggestions"));
    assert!(arg_names.contains(&"auto_generate"));
}

#[test]
fn top_level_has_doctor_and_init() {
    let mut cmd = Cli::command();
    cmd.build();
    let names: Vec<&str> = cmd.get_subcommands().map(|c| c.get_name()).collect();
    assert!(names.contains(&"doctor"), "missing `doctor` subcommand");
    assert!(names.contains(&"init"), "missing `init` subcommand");
}

#[test]
fn doctor_accepts_check_remote_and_series_filter() {
    let mut cmd = Cli::command();
    cmd.build();
    let doctor = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "doctor")
        .unwrap();
    let args: Vec<&str> = doctor
        .get_arguments()
        .map(|a| a.get_id().as_str())
        .collect();
    assert!(args.contains(&"check_remote"));
    assert!(args.contains(&"series"));
}

#[test]
fn snapshots_generate_accepts_refresh_stale() {
    let mut cmd = Cli::command();
    cmd.build();
    let snapshots = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "snapshots")
        .unwrap();
    let generate = snapshots
        .get_subcommands()
        .find(|s| s.get_name() == "generate")
        .unwrap();
    let args: Vec<&str> = generate
        .get_arguments()
        .map(|a| a.get_id().as_str())
        .collect();
    assert!(args.contains(&"refresh_stale"));
}

#[test]
fn init_accepts_rabbitmq_and_force() {
    let mut cmd = Cli::command();
    cmd.build();
    let init = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "init")
        .unwrap();
    let args: Vec<&str> = init.get_arguments().map(|a| a.get_id().as_str()).collect();
    assert!(args.contains(&"config_dir_path"));
    // --snapshot-dir-path is the global flag, exposed via clap's global = true.
    assert!(args.contains(&"snapshot_dir_path"));
    assert!(args.contains(&"rabbitmq_repo_dir_path"));
    assert!(args.contains(&"rabbitmq_branches"));
    assert!(args.contains(&"force"));
}

#[test]
fn init_global_snapshot_dir_short_flag_is_available() {
    // regression: a local --snapshot-dir-path on init shadowed the global -s short flag
    let parsed = Cli::try_parse_from([
        "backhopper",
        "init",
        "-s",
        "/tmp/snaps",
        "--config-dir-path",
        "/tmp/cfg",
    ]);
    assert!(
        parsed.is_ok(),
        "init must accept the global `-s` short flag"
    );
}

#[test]
fn write_suggestions_requires_suggest_projects() {
    let result = Cli::try_parse_from([
        "backhopper",
        "check",
        "patch",
        "--series",
        "rabbitmq-4.1",
        "--write-suggestions",
    ]);
    assert!(
        result.is_err(),
        "--write-suggestions without --suggest-projects must error"
    );
}

#[test]
fn series_sync_advertises_preview_diff_merge_replace() {
    let mut cmd = Cli::command();
    cmd.build();
    let series = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "series")
        .expect("series group missing");
    let sync = series
        .get_subcommands()
        .find(|s| s.get_name() == "sync")
        .expect("series sync subcommand missing");
    let verbs: Vec<&str> = sync.get_subcommands().map(|c| c.get_name()).collect();
    for expected in ["preview", "diff", "merge", "replace"] {
        assert!(
            verbs.contains(&expected),
            "missing series sync verb: {expected}; got {verbs:?}"
        );
    }
}

#[test]
fn series_sync_replace_rejects_overwrite_existing() {
    let r = Cli::try_parse_from([
        "backhopper",
        "series",
        "sync",
        "replace",
        "--from-branch",
        "main",
        "--repo-dir-path",
        "/tmp",
        "--series-name",
        "x",
        "--overwrite-existing",
    ]);
    assert!(
        r.is_err(),
        "replace must not accept --overwrite-existing (merge-only flag)"
    );
}

#[test]
fn snapshots_advertises_introduced_verb() {
    let mut cmd = Cli::command();
    cmd.build();
    let snapshots = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "snapshots")
        .expect("snapshots group missing");
    let verbs: Vec<&str> = snapshots.get_subcommands().map(|c| c.get_name()).collect();
    assert!(
        verbs.contains(&"introduced"),
        "missing snapshots verb: introduced; got {verbs:?}"
    );
}

#[test]
fn snapshots_lookup_no_longer_accepts_all_tags_flag() {
    let r = Cli::try_parse_from([
        "backhopper",
        "snapshots",
        "lookup",
        "--project",
        "ra",
        "--all-tags",
        "--mfa",
        "ra_machine:apply/4",
    ]);
    assert!(r.is_err(), "--all-tags must be gone");
}

#[test]
fn snapshots_lookup_requires_tag() {
    let r = Cli::try_parse_from([
        "backhopper",
        "snapshots",
        "lookup",
        "--project",
        "ra",
        "--mfa",
        "ra_machine:apply/4",
    ]);
    assert!(r.is_err(), "--tag is now required");
}

#[test]
fn snapshots_introduced_requires_at_least_one_mfa() {
    let r = Cli::try_parse_from(["backhopper", "snapshots", "introduced", "--project", "ra"]);
    assert!(r.is_err(), "--mfa is required");
}

#[test]
fn bisect_group_has_commit_verb_with_required_args() {
    let mut cmd = Cli::command();
    cmd.build();
    let bisect = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "bisect")
        .expect("bisect group missing");
    let commit_sub = bisect
        .get_subcommands()
        .find(|s| s.get_name() == "commit")
        .expect("bisect commit subcommand missing");
    let arg_names: Vec<&str> = commit_sub
        .get_arguments()
        .map(|a| a.get_id().as_str())
        .collect();
    for required in ["project", "repo_dir_path", "commit"] {
        assert!(
            arg_names.contains(&required),
            "bisect commit missing arg {required}; got {arg_names:?}"
        );
    }
}

#[test]
fn bisect_commit_rejects_missing_required_flags() {
    let r = Cli::try_parse_from(["backhopper", "bisect", "commit"]);
    assert!(r.is_err(), "bisect commit must require its args");
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

#[test]
fn jigsaw_group_has_the_plan_and_check_verbs() {
    let mut cmd = Cli::command();
    cmd.build();
    let group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "jigsaw")
        .unwrap();
    let names: Vec<&str> = group
        .get_subcommands()
        .map(|c| c.get_name())
        .filter(|n| *n != "help")
        .collect();
    assert_eq!(names, ["plan", "check"]);
}

#[test]
fn jigsaw_plan_requires_a_host_and_a_series() {
    assert!(Cli::try_parse_from(["backhopper", "jigsaw", "plan"]).is_err());
    assert!(Cli::try_parse_from(["backhopper", "jigsaw", "plan", "--host", "rabbitmq"]).is_err());
    assert!(
        Cli::try_parse_from([
            "backhopper",
            "jigsaw",
            "plan",
            "--host",
            "rabbitmq",
            "--series",
            "4.2"
        ])
        .is_ok()
    );
}

#[test]
fn jigsaw_plan_takes_repeated_erlang_majors_and_refuses_a_bad_series() {
    let ok = Cli::try_parse_from([
        "backhopper",
        "jigsaw",
        "plan",
        "--host",
        "rabbitmq",
        "--series",
        "4.2",
        "--erlang-major",
        "27",
        "--erlang-major",
        "28",
    ]);
    assert!(ok.is_ok());
    let bad = Cli::try_parse_from([
        "backhopper",
        "jigsaw",
        "plan",
        "--host",
        "rabbitmq",
        "--series",
        "4",
    ]);
    assert!(bad.is_err());
}

const CHECK: [&str; 3] = ["backhopper", "jigsaw", "check"];

fn check_args(extra: &[&str]) -> Vec<String> {
    CHECK
        .iter()
        .chain(extra)
        .map(|arg| (*arg).to_owned())
        .collect()
}

#[test]
fn jigsaw_check_needs_a_plan_source_and_a_plugin_directory() {
    assert!(Cli::try_parse_from(check_args(&[])).is_err());
    assert!(Cli::try_parse_from(check_args(&["--plan-file-path", "plan.json"])).is_err());
    assert!(
        Cli::try_parse_from(check_args(&[
            "--plan-file-path",
            "plan.json",
            "--plugin-dir-path",
            "plugins"
        ]))
        .is_ok()
    );
}

#[test]
fn jigsaw_check_takes_the_plan_arguments_instead_of_a_plan_file() {
    assert!(
        Cli::try_parse_from(check_args(&[
            "--host",
            "rabbitmq",
            "--series",
            "4.2",
            "--up-to",
            "v4.2.8",
            "--erlang-major",
            "27",
            "--plugin-dir-path",
            "plugins",
            "--plugin-dir-path",
            "vendor"
        ]))
        .is_ok()
    );
    assert!(
        Cli::try_parse_from(check_args(&[
            "--host",
            "rabbitmq",
            "--plugin-dir-path",
            "plugins"
        ]))
        .is_err(),
        "a host without a series"
    );
}

#[test]
fn jigsaw_check_refuses_a_plan_file_together_with_plan_arguments() {
    assert!(
        Cli::try_parse_from(check_args(&[
            "--plan-file-path",
            "plan.json",
            "--host",
            "rabbitmq",
            "--series",
            "4.2",
            "--plugin-dir-path",
            "plugins"
        ]))
        .is_err()
    );
    assert!(
        Cli::try_parse_from(check_args(&[
            "--plan-file-path",
            "plan.json",
            "--erlang-major",
            "27",
            "--plugin-dir-path",
            "plugins"
        ]))
        .is_err()
    );
}

#[test]
fn the_jigsaw_group_and_its_verb_accept_abbreviations() {
    let abbreviated = Cli::try_parse_from([
        "backhopper",
        "jig",
        "pl",
        "--host",
        "rabbitmq",
        "--series",
        "4.2",
    ]);
    assert!(abbreviated.is_ok());
}

#[test]
fn snapshots_generate_accepts_from_jigsaw_plan_alone() {
    let alone = Cli::try_parse_from([
        "backhopper",
        "snapshots",
        "generate",
        "--from-jigsaw-plan",
        "plan.json",
    ]);
    assert!(alone.is_ok());
    for conflicting in [
        ["--project", "ra"],
        ["--since", "v1.0.0"],
        ["--series", "rabbitmq-4.2"],
    ] {
        let mut argv = vec![
            "backhopper",
            "snapshots",
            "generate",
            "--from-jigsaw-plan",
            "plan.json",
        ];
        argv.extend(conflicting);
        assert!(Cli::try_parse_from(argv).is_err(), "{conflicting:?}");
    }
}
