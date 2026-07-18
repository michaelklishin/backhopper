// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Subcommand;

use backhopper_core::model::names::{CommitShaPrefix, ProjectName};

#[derive(Debug, Subcommand)]
pub enum BisectCmd {
    /// Walk every stored tag of a project and report the newest tag at
    /// which the commit's verdict is still `Compatible`, plus the tag
    /// where it flips to `Incompatible`.
    Commit {
        #[arg(long)]
        project: ProjectName,
        #[arg(long, default_value = ".")]
        repo_dir_path: PathBuf,
        /// Skip the verdict cache for this invocation, for both reads
        /// and writes.
        #[arg(long)]
        no_cache: bool,
        #[arg(
            value_name = "COMMIT_SHA",
            long_help = crate::cli::COMMIT_SHA_HELP
        )]
        commit: CommitShaPrefix,
    },
}
