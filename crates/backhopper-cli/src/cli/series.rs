// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use backhopper_core::model::names::SeriesName;

#[derive(Debug, Subcommand)]
pub enum SeriesCmd {
    /// List configured series.
    List,
    /// Show the pins for a series.
    Show {
        #[arg(long)]
        series: SeriesName,
    },
    /// Build a `[[series]]` stanza from a RabbitMQ branch's `rabbitmq-components.mk`.
    #[command(subcommand)]
    Sync(SyncCmd),
}

#[derive(Debug, Args)]
pub struct SyncCommon {
    /// Branch name, tag, or any rev `gix` can resolve.
    #[arg(long)]
    pub from_branch: String,
    #[arg(long)]
    pub repo_dir_path: PathBuf,
    #[arg(long)]
    pub series_name: SeriesName,
}

#[derive(Debug, Subcommand)]
pub enum SyncCmd {
    /// Print the inferred `[[series]]` stanza. Does not write.
    Preview {
        #[command(flatten)]
        common: SyncCommon,
    },
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
