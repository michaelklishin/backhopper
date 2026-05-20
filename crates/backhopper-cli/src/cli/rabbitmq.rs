// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum RabbitmqCmd {
    /// Infer a series block from `rabbitmq-components.mk` in a RabbitMQ
    /// checkout. Writes a TOML `[[series]]` block to stdout.
    #[command(name = "infer_series")]
    InferSeries {
        #[arg(long)]
        repo_dir_path: PathBuf,
        #[arg(long, conflicts_with = "all_branches")]
        branch: Option<String>,
        #[arg(long, conflicts_with = "branch")]
        all_branches: bool,
        #[arg(
            long,
            value_delimiter = ',',
            default_values_t = default_branches(),
            help = "Branches to walk when --all-branches (comma-separated)",
        )]
        branches: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, help = "Print warnings for skipped or commit-pinned deps")]
        show_skipped: bool,
    },
}

fn default_branches() -> Vec<String> {
    vec![
        "main".into(),
        "v4.3.x".into(),
        "v4.2.x".into(),
        "v4.1.x".into(),
        "v4.0.x".into(),
    ]
}
