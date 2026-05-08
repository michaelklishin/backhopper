//! `backhopper` CLI library: argument parsing and command dispatch.

pub mod cli;
pub mod commands;
pub mod errors;
pub mod output;

use std::process::ExitCode;

use clap::Parser;

pub use cli::{Cli, GlobalArgs, Group};
pub use errors::{CliError, CliResult};

pub fn run() -> CliResult<ExitCode> {
    let cli = Cli::parse();
    init_logging(&cli.global);
    let exit = commands::dispatch(cli)?;
    Ok(ExitCode::from(exit as u8))
}

fn init_logging(args: &GlobalArgs) {
    let level = if args.quiet {
        "error"
    } else if args.verbose {
        "debug"
    } else {
        "warn"
    };
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| level.to_owned());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .try_init();
}
