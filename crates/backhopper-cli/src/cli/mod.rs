// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Clap-based command-line parser definition.
//!
//! Per-group argument shapes live in sibling modules (`projects`, `series`,
//! `snapshots`, …) so each command's surface area is local to one file.

use std::path::PathBuf;

use bel7_cli::TableStyle;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum, crate_version};

pub mod bisect;
pub mod check;
pub mod config;
pub mod doctor;
pub mod init;
pub mod projects;
pub mod rev;
pub mod schema;
pub mod series;
pub mod shell;
pub mod snapshots;
pub mod suites;
pub mod tree_source;
pub mod xref;

pub use bisect::BisectCmd;
pub use check::{CheckCmd, CheckFlags, SourcePinArgs};
pub use config::ConfigCmd;
pub use doctor::DoctorCmd;
pub use init::InitCmd;
pub use projects::ProjectsCmd;
pub use rev::RevCmd;
pub use schema::{SchemaCmd, SchemaDiffArgs, SchemaShowArgs};
pub use series::{PreviewArgs, SeriesCmd, SyncCmd, SyncCommon};
pub use shell::{CompletionsCmd, ShellCmd};
pub use snapshots::SnapshotsCmd;
pub use suites::SuitesCmd;
pub use tree_source::TreeSource;
pub use xref::XrefCmd;

// CARGO_PKG_NAME is "backhopper-cli"; the user-facing binary is "backhopper".
#[derive(Debug, Parser)]
#[command(
    name = "backhopper",
    version = crate_version!(),
    about = "Record Erlang and Elixir public APIs per git tag and answer compatibility questions",
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
        help = "Path to the backhopper.toml file"
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
        default_value_t = Formatter::Json,
        value_enum,
        help = "Output formatter (default: json; pass `--formatter text` for human-readable tables)",
    )]
    pub formatter: Formatter,

    #[arg(
        long,
        short = 'q',
        global = true,
        conflicts_with = "verbose",
        help = "Suppress progress output; errors still print to stderr"
    )]
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
    Markdown,
    /// JSONL projection: one `SummaryRow` per line, no array wrap.
    Summary,
    /// Tab-separated projection: one row per result.
    TextSummary,
}

#[derive(Debug, Subcommand)]
pub enum Group {
    /// Print a single-shot workspace health summary (config + per-series pin coverage).
    Doctor(DoctorCmd),
    /// Bootstrap a starter `backhopper.toml` in the current directory (or a chosen path).
    Init(InitCmd),
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
    /// Bisect across stored tags to find the verdict-flip point.
    Bisect {
        #[command(subcommand)]
        cmd: BisectCmd,
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
    /// Inspect the wire-format JSON schema this binary embeds.
    Schema {
        #[command(subcommand)]
        cmd: SchemaCmd,
    },
    /// Resolve commit-SHA prefixes against a git repo.
    Rev {
        #[command(subcommand)]
        cmd: RevCmd,
    },
    /// Print the `backhopper` version.
    Version,
}
