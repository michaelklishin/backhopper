// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Coverage for the `bel7-cli` integration: exit-code mapping via
//! `ExitCodeProvider`, `TableStyle` enum wiring, and completion-shell parsing.

use std::path::PathBuf;
use std::str::FromStr;

use bel7_cli::{CompletionShell, ExitCode, ExitCodeExt, ExitCodeProvider, TableStyle, codes};
use clap::{CommandFactory, Parser};

use backhopper_cli::{Cli, CliError};

#[test]
fn cli_error_implements_exit_code_provider() {
    let e = CliError::InvalidInput("bad input".into());
    let via_trait: ExitCode = <CliError as ExitCodeProvider>::exit_code(&e);
    let via_inherent: ExitCode = e.exit_code();
    assert_eq!(via_trait, via_inherent);
    assert_eq!(via_trait, codes::USAGE);
}

#[test]
fn exit_code_ext_to_i32_matches_sysexits_constants() {
    assert_eq!(codes::OK.to_i32(), 0);
    assert_eq!(codes::USAGE.to_i32(), 64);
    assert_eq!(codes::NO_INPUT.to_i32(), 66);
    assert_eq!(codes::IO_ERR.to_i32(), 74);
    assert_eq!(codes::DATA_ERR.to_i32(), 65);
    assert_eq!(codes::SOFTWARE.to_i32(), 70);
}

#[test]
fn config_not_found_uses_no_input_via_trait() {
    let e = CliError::ConfigNotFound {
        tried: vec![PathBuf::from("./backhopper.toml")],
    };
    assert_eq!(
        <CliError as ExitCodeProvider>::exit_code(&e),
        codes::NO_INPUT
    );
}

#[test]
fn cli_parses_table_style_flag_into_bel7_enum() {
    let parsed = Cli::try_parse_from(["backhopper", "--table-style", "markdown", "version"])
        .expect("--table-style markdown should parse");
    assert_eq!(parsed.global.table_style, TableStyle::Markdown);
}

#[test]
fn cli_rejects_invalid_table_style_value() {
    let err = Cli::try_parse_from(["backhopper", "--table-style", "not_a_real_style", "version"])
        .expect_err("invalid table style should fail parsing");
    let msg = err.to_string();
    assert!(
        msg.contains("invalid value") || msg.contains("possible values"),
        "got: {msg}"
    );
}

#[test]
fn completions_subcommand_accepts_every_bel7_shell_value() {
    for shell in ["bash", "zsh", "fish", "elvish", "nushell", "powershell"] {
        Cli::try_parse_from(["backhopper", "shell", "completions", shell])
            .unwrap_or_else(|e| panic!("shell completions {shell} should parse: {e}"));
    }
}

#[test]
fn completions_shell_aliases_match_bel7_definitions() {
    let nu = CompletionShell::from_str("nu").expect("nu alias should parse");
    assert_eq!(nu, CompletionShell::Nushell);
    let pwsh = CompletionShell::from_str("pwsh").expect("pwsh alias should parse");
    assert_eq!(pwsh, CompletionShell::PowerShell);
}

#[test]
fn completions_shell_arg_is_optional_for_env_detection() {
    let cli =
        Cli::try_parse_from(["backhopper", "shell", "completions"]).expect("shell is optional");
    if let backhopper_cli::Group::Shell { cmd } = cli.group {
        match cmd {
            backhopper_cli::ShellCmd::Completions(c) => {
                assert!(c.shell.is_none(), "no explicit shell should leave None");
            }
        }
    } else {
        panic!("expected Group::Shell");
    }
}

#[test]
fn table_style_default_is_modern_when_flag_absent() {
    let cli = Cli::try_parse_from(["backhopper", "version"]).expect("default parse");
    assert_eq!(cli.global.table_style, TableStyle::Modern);
}

#[test]
fn cli_command_advertises_global_table_style_argument() {
    let mut cmd = Cli::command();
    cmd.build();
    let arg = cmd
        .get_arguments()
        .find(|a| a.get_id().as_str() == "table_style")
        .expect("table_style global flag missing");
    assert!(arg.is_global_set(), "table_style should be global");
}