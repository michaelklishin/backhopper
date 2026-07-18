// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Subcommand;

use backhopper_core::model::names::CommitShaPrefix;

#[derive(Debug, Subcommand)]
pub enum RevCmd {
    /// Expand a SHA prefix (7 to 40 hex characters) to its full
    /// 40-character commit SHA. Resolves against the repo at
    /// `--repo-dir-path` via `gix::rev_parse_single`.
    Resolve {
        #[arg(
            long,
            default_value = ".",
            help = "Path to the git working tree or bare repo the prefix resolves against"
        )]
        repo_dir_path: PathBuf,
        #[arg(
            long,
            help = "Include the commit subject in the output payload (one extra git lookup)"
        )]
        subject: bool,
        #[arg(
            value_name = "COMMIT_SHA",
            long_help = crate::cli::COMMIT_SHA_PREFIX_HELP
        )]
        commit: CommitShaPrefix,
    },
}
