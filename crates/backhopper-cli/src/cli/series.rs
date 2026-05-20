// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Subcommand;

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
    Sync {
        /// Branch name, tag, or any rev `gix` can resolve.
        #[arg(long)]
        from_branch: String,
        #[arg(long)]
        repo_dir_path: PathBuf,
        #[arg(long)]
        series_name: SeriesName,
    },
}
