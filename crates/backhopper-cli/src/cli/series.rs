// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use backhopper_core::model::names::SeriesName;

pub const DEFAULT_BRANCHES: &[&str] = &["main", "v4.3.x", "v4.2.x", "v4.1.x", "v4.0.x"];

pub fn default_branches() -> Vec<String> {
    DEFAULT_BRANCHES.iter().map(|s| (*s).to_owned()).collect()
}

#[derive(Debug, Subcommand)]
pub enum SeriesCmd {
    /// List configured series.
    List,
    /// Show the pins for a series.
    Show {
        #[arg(long)]
        series: SeriesName,
    },
    /// Read the dep pins from a branch's `rabbitmq-components.mk`.
    Pins(PinsArgs),
    /// Build a `[[series]]` stanza from a RabbitMQ branch's `rabbitmq-components.mk`.
    #[command(subcommand)]
    Sync(SyncCmd),
}

#[derive(Debug, Args)]
pub struct PinsArgs {
    /// Branch name, tag, or any rev `gix` can resolve.
    #[arg(long)]
    pub branch: String,
    #[arg(long)]
    pub repo_dir_path: PathBuf,
    /// Compare against a second branch and report pin divergence
    /// instead of the raw pin list.
    #[arg(long)]
    pub against_branch: Option<String>,
}

#[derive(Debug, Args)]
pub struct SyncCommon {
    /// Branch name, tag, or any rev `gix` can resolve.
    #[arg(long)]
    pub from_branch: String,
    #[arg(long)]
    pub repo_dir_path: PathBuf,
    /// Defaults to a name derived from `--from-branch` (`main` -> `rabbitmq-main`,
    /// `v4.2.x` -> `rabbitmq-4.2`, etc.).
    #[arg(long)]
    pub series_name: Option<SeriesName>,
}

#[derive(Debug, Args)]
pub struct PreviewArgs {
    /// Branch name, tag, or any rev `gix` can resolve. Required unless
    /// `--all-branches` or `--branches` is set.
    #[arg(
        long,
        conflicts_with_all = ["all_branches", "branches"],
        required_unless_present_any = ["all_branches", "branches"],
    )]
    pub from_branch: Option<String>,
    #[arg(long)]
    pub repo_dir_path: PathBuf,
    /// Defaults to a name derived from the single resolved branch. Rejected
    /// when combined with multi-branch flags.
    #[arg(long, conflicts_with_all = ["all_branches", "branches"])]
    pub series_name: Option<SeriesName>,
    /// Walk the default branch list (`main`, `v4.3.x`, `v4.2.x`, `v4.1.x`, `v4.0.x`).
    #[arg(long, conflicts_with = "branches")]
    pub all_branches: bool,
    /// Walk an explicit comma-separated branch list.
    #[arg(long, value_delimiter = ',')]
    pub branches: Vec<String>,
    /// Emit `# skipped <dep>: <reason>` lines for deps with invalid project
    /// names or invalid tag names.
    #[arg(long)]
    pub show_skipped: bool,
}

#[derive(Debug, Subcommand)]
pub enum SyncCmd {
    /// Print the inferred `[[series]]` stanza(s). Does not write.
    Preview(PreviewArgs),
    /// Print a unified diff of how `merge` or `replace` would change the config. Does not write.
    Diff {
        #[command(flatten)]
        common: SyncCommon,
        /// Preview the `replace` operation instead of the additive merge.
        #[arg(long)]
        replace: bool,
        /// Preview the merge with conflicting pins overwritten.
        #[arg(long, conflicts_with = "replace")]
        overwrite_existing: bool,
    },
    /// Add new pins to the existing `[[series]]` block. Existing pins are kept; tag conflicts are
    /// reported and skipped unless `--overwrite-existing` is set.
    Merge {
        #[command(flatten)]
        common: SyncCommon,
        /// Replace the tag of any pin whose inferred value differs.
        #[arg(long)]
        overwrite_existing: bool,
    },
    /// Rewrite the named `[[series]]` block, dropping pins not present in the inferred set.
    Replace {
        #[command(flatten)]
        common: SyncCommon,
    },
}
