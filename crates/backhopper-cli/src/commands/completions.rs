use std::io;

use clap::CommandFactory;
use clap_complete::Shell;
use clap_complete::generate;

use crate::cli::{Cli, CompletionsCmd, GlobalArgs};
use crate::errors::CliResult;

pub fn handle(_args: &GlobalArgs, cmd: CompletionsCmd) -> CliResult<i32> {
    let mut cli = Cli::command();
    let bin = "backhopper";
    let mut stdout = io::stdout().lock();
    match cmd {
        CompletionsCmd::Bash => generate(Shell::Bash, &mut cli, bin, &mut stdout),
        CompletionsCmd::Zsh => generate(Shell::Zsh, &mut cli, bin, &mut stdout),
        CompletionsCmd::Fish => generate(Shell::Fish, &mut cli, bin, &mut stdout),
        CompletionsCmd::Nushell => {
            generate(clap_complete_nushell::Nushell, &mut cli, bin, &mut stdout)
        }
        CompletionsCmd::Pwsh => generate(Shell::PowerShell, &mut cli, bin, &mut stdout),
    }
    Ok(0)
}
