// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use backhopper_core::model::names::{ProjectName, SeriesName, TagName};

#[derive(Debug, Args, Clone, Copy, Default)]
pub struct CheckOutputFlags {
    #[arg(
        long,
        help = "Print untracked module calls in the text-mode footer (informational, not a verdict input)"
    )]
    pub show_untracked_calls: bool,
    #[arg(
        long,
        help = "Include OTP stdlib calls in the untracked-calls footer (implies --show-untracked-calls)"
    )]
    pub show_otp_calls: bool,
    #[arg(
        long,
        help = "Print only the counts: 'compatible=N requires_adaptation=N incompatible=N'"
    )]
    pub summary_only: bool,
    #[arg(
        long,
        help = "Resolve each untracked module's `.erl` against the target checkout; absent files flip the verdict to Incompatible"
    )]
    pub resolve_untracked_modules: bool,
    #[arg(
        long,
        help = "Print each tracked call site that contributed to the per-pin `tracked symbols referenced` count"
    )]
    pub explain: bool,
}

/// Optional source-side pin descriptor. When set, the analyzer diffs
/// specs and record field lists per touched MFA or record between the
/// source pin and the target pin, surfacing `SignatureChanged` and
/// `RecordFieldsChanged` reasons that snapshot-export-presence checks
/// alone would miss.
#[derive(Debug, Args, Clone, Default)]
pub struct SourcePinArgs {
    /// Source-side tag for the single-pin case (paired with the target's
    /// `--project` and `--tag`). Same project.
    #[arg(long)]
    pub source_tag: Option<TagName>,
    /// Source-side series for the series case (paired with the target's
    /// `--series`). Project names are matched between target and source.
    #[arg(long)]
    pub source_series: Option<SeriesName>,
}

#[derive(Debug, Subcommand)]
pub enum CheckCmd {
    /// Check a unified-diff patch. The patch comes from a file or stdin.
    Patch {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        tag: Option<TagName>,
        #[arg(long)]
        series: Option<SeriesName>,
        #[arg(
            long,
            help = "Target checkout for resolving untracked modules. Required when --resolve-untracked-modules is set"
        )]
        repo_dir_path: Option<PathBuf>,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        diagnostics: CheckOutputFlags,
        #[arg(value_name = "PATCH_FILE_PATH")]
        patch_file_path: Option<PathBuf>,
    },
    /// Check a single commit (the diff against its parent).
    Commit {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        tag: Option<TagName>,
        #[arg(long)]
        series: Option<SeriesName>,
        #[arg(long, default_value = ".")]
        repo_dir_path: PathBuf,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        diagnostics: CheckOutputFlags,
        #[arg(value_name = "COMMIT_SHA")]
        commit: String,
    },
    /// Check a commit range or the diff of a merge commit.
    Range {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        tag: Option<TagName>,
        #[arg(long)]
        series: Option<SeriesName>,
        #[arg(long, default_value = ".")]
        repo_dir_path: PathBuf,
        #[arg(long, conflicts_with = "merge_commit")]
        range: Option<String>,
        #[arg(long)]
        merge_commit: Option<String>,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        diagnostics: CheckOutputFlags,
    },
    /// Evaluate many commits against one or more series. One row per
    /// (commit, series) pair. Blank lines and `#` comments in the
    /// commits file are skipped.
    Batch {
        /// Series to evaluate against. Repeat the flag or use a
        /// comma-separated list.
        #[arg(long, required = true, value_delimiter = ',')]
        series: Vec<SeriesName>,
        #[arg(long, default_value = ".")]
        repo_dir_path: PathBuf,
        /// File of one SHA or rev-spec per line; `-` reads stdin.
        #[arg(long, required = true)]
        commits_file_path: PathBuf,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        diagnostics: CheckOutputFlags,
    },
}