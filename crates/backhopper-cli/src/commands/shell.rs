// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use crate::outcome::CommandOutcome;
use std::io;

use bel7_cli::{CompletionShell, generate_completions};
use clap::CommandFactory;

use crate::cli::{Cli, CompletionsCmd, GlobalArgs, ShellCmd};
use crate::errors::CliResult;

pub fn handle(_args: &GlobalArgs, cmd: ShellCmd) -> CliResult<CommandOutcome> {
    match cmd {
        ShellCmd::Completions(c) => completions(c),
    }
}

fn completions(cmd: CompletionsCmd) -> CliResult<CommandOutcome> {
    let shell = cmd.shell.unwrap_or_else(CompletionShell::detect);
    let mut cli = Cli::command();
    let mut stdout = io::stdout().lock();
    generate_completions(shell, &mut cli, "backhopper", &mut stdout);
    Ok(CommandOutcome::Success)
}
