// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end coverage for the `bel7-cli` adoption: completion-shell
//! values, table-style values, and an invalid-input exit code wired
//! through `ExitCodeProvider`.

use super::helpers::cli::{run_fails, run_succeeds, stderr, stdout};

#[test]
fn completions_bash_emits_a_bash_completion_script() {
    let out = stdout(&run_succeeds(["shell", "completions", "bash"]));
    assert!(
        out.contains("_backhopper") || out.contains("complete -F"),
        "expected a bash completion body, got: {out}"
    );
}

#[test]
fn completions_zsh_emits_a_zsh_completion_script() {
    let out = stdout(&run_succeeds(["shell", "completions", "zsh"]));
    assert!(
        out.contains("#compdef") && out.contains("backhopper"),
        "expected a zsh `#compdef backhopper` header, got: {out}"
    );
}

#[test]
fn completions_fish_emits_a_fish_completion_script() {
    let out = stdout(&run_succeeds(["shell", "completions", "fish"]));
    assert!(
        out.contains("complete -c backhopper"),
        "expected fish completion, got: {out}"
    );
}

#[test]
fn completions_elvish_is_advertised_via_bel7_cli() {
    let out = stdout(&run_succeeds(["shell", "completions", "elvish"]));
    assert!(!out.is_empty(), "expected non-empty elvish script");
}

#[test]
fn completions_powershell_alias_pwsh_is_accepted() {
    let out = stdout(&run_succeeds(["shell", "completions", "pwsh"]));
    assert!(
        out.contains("backhopper"),
        "expected the binary name in the completion, got: {out}"
    );
}

#[test]
fn completions_rejects_unknown_shell_value() {
    let assert = run_fails(["shell", "completions", "tcsh"]);
    let err = stderr(&assert);
    assert!(
        err.contains("invalid value") || err.contains("possible values"),
        "expected clap to reject `tcsh`, got: {err}"
    );
}

#[test]
fn table_style_markdown_is_accepted_at_parse_time() {
    let out = stdout(&run_succeeds(["--table-style", "markdown", "version"]));
    assert!(
        out.contains("backhopper"),
        "expected version output to mention the binary name, got: {out}"
    );
}

#[test]
fn table_style_invalid_value_returns_usage_exit_code() {
    let assert = run_fails(["--table-style", "not_real", "version"]);
    let err = stderr(&assert);
    assert!(
        err.contains("invalid value"),
        "expected `invalid value` clap error, got: {err}"
    );
}

#[test]
fn missing_required_subcommand_returns_nonzero_exit() {
    run_fails(["projects"]);
}

#[test]
fn version_command_prints_a_single_line_with_semver() {
    let out = stdout(&run_succeeds(["version"]));
    let trimmed = out.trim_end_matches('\n');
    assert_eq!(
        trimmed.lines().count(),
        1,
        "version should be one line: {trimmed:?}"
    );
    assert!(
        trimmed.contains("backhopper "),
        "version should mention backhopper: {trimmed:?}"
    );
}

#[test]
fn no_arguments_prints_help_and_returns_nonzero_exit() {
    let assert = run_fails::<[&str; 0], _>([]);
    let err = stderr(&assert);
    assert!(
        err.contains("Usage:") || err.contains("USAGE"),
        "expected a usage header on stderr, got: {err}"
    );
}
