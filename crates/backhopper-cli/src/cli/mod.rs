//! Clap-based command-line parser definition.
//!
//! Per-group argument shapes live in sibling modules (`projects`, `series`,
//! `snapshots`, …) so each command's surface area is local to one file.

use std::path::PathBuf;

use bel7_cli::TableStyle;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

pub mod check;
pub mod config;
pub mod projects;
pub mod rabbitmq;
pub mod series;
pub mod shell;
pub mod snapshots;
pub mod suites;
pub mod tree_source;
pub mod xref;

pub use check::{CheckCmd, CheckOutputFlags, SourcePinArgs};
pub use config::ConfigCmd;
pub use projects::ProjectsCmd;
pub use rabbitmq::RabbitmqCmd;
pub use series::SeriesCmd;
pub use shell::{CompletionsCmd, ShellCmd};
pub use snapshots::SnapshotsCmd;
pub use suites::SuitesCmd;
pub use tree_source::TreeSource;
pub use xref::XrefCmd;

// `CARGO_PKG_NAME` is "backhopper-cli"; the user-facing binary is "backhopper".
#[derive(Debug, Parser)]
#[command(
    name = "backhopper",
    version = env!("CARGO_PKG_VERSION"),
    about = "Record Erlang/Elixir public APIs per git tag and answer compatibility questions",
    propagate_version = true,
    arg_required_else_help = true,
    infer_subcommands = true,
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub group: Group,
}

#[derive(Debug, Args, Clone)]
pub struct GlobalArgs {
    #[arg(
        long,
        short = 'c',
        env = "BACKHOPPER_CONFIG_FILE_PATH",
        global = true,
        help = "Path to backhopper.toml"
    )]
    pub config_file_path: Option<PathBuf>,

    #[arg(
        long,
        short = 's',
        env = "BACKHOPPER_SNAPSHOT_DIR_PATH",
        global = true,
        help = "Override the snapshot directory; takes precedence over the config's snapshot_dir"
    )]
    pub snapshot_dir_path: Option<PathBuf>,

    #[arg(
        long,
        env = "BACKHOPPER_FORMATTER",
        global = true,
        default_value_t = Formatter::Text,
        value_enum,
        help = "Output formatter",
    )]
    pub formatter: Formatter,

    #[arg(long, short = 'q', global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    #[arg(
        long,
        short = 'v',
        global = true,
        conflicts_with = "quiet",
        action = ArgAction::Count,
        help = "Increase log verbosity: -v=info, -vv=debug, -vvv=trace"
    )]
    pub verbose: u8,

    #[arg(
        long,
        env = "BACKHOPPER_NON_INTERACTIVE_MODE",
        global = true,
        help = "Disable prompts and TTY-only behaviors"
    )]
    pub non_interactive: bool,

    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t = TableStyle::Modern,
        help = "Style preset for the text formatter"
    )]
    pub table_style: TableStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Formatter {
    Json,
    Text,
}

#[derive(Debug, Subcommand)]
pub enum Group {
    /// Inspect configured projects.
    Projects {
        #[command(subcommand)]
        cmd: ProjectsCmd,
    },
    /// Inspect configured release series.
    Series {
        #[command(subcommand)]
        cmd: SeriesCmd,
    },
    /// Manage and query API snapshots.
    Snapshots {
        #[command(subcommand)]
        cmd: SnapshotsCmd,
    },
    /// Check compatibility of patches, commits, ranges, and batches.
    Check {
        #[command(subcommand)]
        cmd: CheckCmd,
    },
    /// Inspect or validate the loaded configuration file.
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Shell-related helpers (completion scripts).
    Shell {
        #[command(subcommand)]
        cmd: ShellCmd,
    },
    /// Whole-program cross-reference queries over an Erlang source tree.
    Xref {
        #[command(subcommand)]
        cmd: XrefCmd,
    },
    /// Test-suite selection for RabbitMQ-style backport batches.
    Suites {
        #[command(subcommand)]
        cmd: SuitesCmd,
    },
    /// RabbitMQ-specific commands.
    Rabbitmq {
        #[command(subcommand)]
        cmd: RabbitmqCmd,
    },
    /// Print the `backhopper` version.
    Version,
}
