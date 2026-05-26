// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Args;

/// Write a starter `backhopper.toml`. With `--rabbitmq`, also walks the
/// supplied RabbitMQ checkout and writes one `[[series]]` block per branch
/// inferred from `rabbitmq-components.mk`.
#[derive(Debug, Args, Clone)]
pub struct InitCmd {
    /// Directory in which to write `backhopper.toml`. Defaults to the
    /// current directory. The file path is `<config-dir>/backhopper.toml`.
    #[arg(long)]
    pub config_dir_path: Option<PathBuf>,

    /// Path to a RabbitMQ checkout. When set, every branch in
    /// `--rabbitmq-branches` is parsed for `rabbitmq-components.mk` and
    /// written as a `[[series]]` block. Project blocks for each pinned dep
    /// are also stubbed out (with `git_url = "TODO"`).
    #[arg(long)]
    pub rabbitmq_repo_dir_path: Option<PathBuf>,

    /// Branches to scan in the RabbitMQ checkout. Comma-separated, defaults
    /// to the current supported set.
    #[arg(long, value_delimiter = ',', default_values_t = crate::cli::series::default_branches())]
    pub rabbitmq_branches: Vec<String>,

    /// Overwrite an existing `backhopper.toml`. Without this flag, an
    /// existing file is left untouched and `init` exits non-zero.
    #[arg(long)]
    pub force: bool,
}
