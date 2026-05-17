//! `backhopper` CLI library: argument parsing and command dispatch.

pub mod cli;
pub mod commands;
pub mod errors;
pub mod output;
pub mod tables;

use std::env;
use std::io;

use bel7_cli::should_colorize_stderr;
use clap::Parser;
use tracing_subscriber::EnvFilter;

pub use cli::{Cli, CompletionsCmd, GlobalArgs, Group, ShellCmd};
pub use errors::{CliError, CliResult};

pub fn run() -> CliResult<i32> {
    let cli = Cli::parse();
    init_logging(&cli.global);
    commands::dispatch(cli)
}

fn init_logging(args: &GlobalArgs) {
    let level = if args.quiet {
        "error"
    } else {
        match args.verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = env::var("RUST_LOG").unwrap_or_else(|_| level.to_owned());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .with_writer(io::stderr)
        .with_ansi(should_colorize_stderr())
        .try_init();
}
