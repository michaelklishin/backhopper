// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use backhopper_core::model::names::{GitRef, SeriesName};

#[derive(Debug, Subcommand)]
pub enum SiblingsCmd {
    /// Rank sibling-branch commits that look like they should have
    /// cascaded to a series but never did.
    Doctor(SiblingsDoctorArgs),
}

#[derive(Debug, Args)]
pub struct SiblingsDoctorArgs {
    /// Series whose self-pin names the target branch.
    #[arg(long)]
    pub series: SeriesName,

    /// Branches to walk for unswept fixes. Repeat the flag or use a
    /// comma-separated list.
    #[arg(long = "source-branch", value_delimiter = ',', default_value = "main")]
    pub source_branches: Vec<GitRef>,

    /// Target branch override for a series without a self-pin (or to
    /// inspect another branch of the same repo).
    #[arg(long)]
    pub target_branch: Option<GitRef>,

    /// Working repo holding both the source and target branches. A
    /// `repo_dir_path` on the series' self-pin wins over this flag.
    #[arg(long, default_value = ".")]
    pub repo_dir_path: PathBuf,

    /// Tag or commit SHA bounding the walk. Default: the last release
    /// tag reachable from the target branch.
    #[arg(long, value_name = "SHA|TAG")]
    pub since: Option<String>,

    /// Keep only the N highest-confidence candidates.
    #[arg(long, default_value_t = 20)]
    pub top: usize,

    /// Add a branches column to the text table (one containment walk
    /// per local branch).
    #[arg(long)]
    pub with_branches: bool,

    /// Replace the family vocabulary with terms from a file: one term
    /// per line, blank lines and `#` comments skipped.
    #[arg(long)]
    pub vocabulary_file_path: Option<PathBuf>,

    /// Include the per-factor score breakdown on every candidate.
    #[arg(long)]
    pub explain: bool,

    /// Skip the on-disk candidate cache for this invocation.
    #[arg(long)]
    pub no_cache: bool,
}
