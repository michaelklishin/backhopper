use bel7_cli::CompletionShell;
use clap::{Args, Subcommand};

#[derive(Debug, Args, Clone)]
pub struct CompletionsCmd {
    /// Target shell. If omitted, detected from the environment.
    #[arg(value_enum)]
    pub shell: Option<CompletionShell>,
}

#[derive(Debug, Subcommand)]
pub enum ShellCmd {
    /// Print a shell-completion script. Detects the shell when omitted.
    Completions(CompletionsCmd),
}
