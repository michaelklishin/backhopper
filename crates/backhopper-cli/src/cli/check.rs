// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use backhopper_core::model::names::{CommitShaPrefix, ProjectName, SeriesName, TagName};

#[derive(Debug, Args, Clone, Copy, Default)]
pub struct CheckFlags {
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
        help = "Print only the counts: `compatible=N requires_adaptation=N incompatible=N`"
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
    #[arg(
        long,
        help = "Group untracked calls by inferred project and emit a ready-to-paste `[[project]]` TOML stub for each candidate"
    )]
    pub suggest_projects: bool,
    #[arg(
        long,
        requires = "suggest_projects",
        help = "Append the suggested `[[project]]` stubs to the loaded backhopper.toml (each stub leaves git_url as a TODO marker)"
    )]
    pub write_suggestions: bool,
    #[arg(
        long,
        help = "Generate any missing pin snapshots before evaluating, instead of failing with `snapshots missing` (the default)"
    )]
    pub auto_generate: bool,
    /// Write one JSON line {summary, pins, scope, exit} to stdout, suppress
    /// the standard JSON envelope, then exit. For agent pipelines that only
    /// need the verdict and the exit code.
    #[arg(long)]
    pub terse: bool,
    /// For each `MissingPrereq` against a self-pin, run `git log -S <name>` to
    /// suggest a candidate prerequisite commit. Off by default: slow on large
    /// histories.
    #[arg(long)]
    pub suggest_prereqs: bool,
    /// Skip the verdict cache for this invocation, for both reads
    /// and writes.
    #[arg(long)]
    pub no_cache: bool,
}

/// Cross-branch target-repo flags. When `target_repo_dir_path` is set,
/// the verdict pipeline classifies every touched path against the
/// target tree and emits `PathRename` or `PathsMissingOnTarget` rows
/// per doc 014.
#[derive(Debug, Args, Clone, Default)]
pub struct TargetRepoArgs {
    /// Working clone of the branch the commit is being backported to.
    /// Triggers the cross-branch analyser. Absent: legacy behaviour.
    #[arg(long)]
    pub target_repo_dir_path: Option<PathBuf>,
    /// Ref to resolve inside the target repo. Defaults to `HEAD`.
    #[arg(long, default_value = "HEAD", requires = "target_repo_dir_path")]
    pub target_ref: String,
    /// External `[[path_translation]]` TOML file unioned with the
    /// stanzas in `backhopper.toml`. The flag accepts a path; missing
    /// file is a hard error.
    #[arg(long)]
    pub path_translations_file_path: Option<PathBuf>,
    /// Cap the target-branch first-parent walk used by already-present
    /// detection. A backstop, not a tuning knob: real divergence
    /// windows are a few hundred commits.
    #[arg(long, default_value_t = 5000, requires = "target_repo_dir_path")]
    pub target_walk_limit: usize,
    /// Skip already-present detection against the target branch.
    #[arg(long, requires = "target_repo_dir_path")]
    pub skip_already_present: bool,
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
            default_value = ".",
            help = "Target checkout for resolving untracked modules. Used when `--resolve-untracked-modules` is set"
        )]
        repo_dir_path: PathBuf,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
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
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
        #[arg(
            value_name = "COMMIT_SHA",
            long_help = "Commit SHA (7 to 40 hex characters; case-insensitive)"
        )]
        commit: CommitShaPrefix,
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
        #[arg(
            long,
            long_help = "Merge commit SHA (7 to 40 hex characters; case-insensitive)"
        )]
        merge_commit: Option<CommitShaPrefix>,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
    },
    /// Check the diff of a merge commit, equivalent to `SHA^2..SHA^1`.
    /// Use this when a single-arg `check commit <MERGE-SHA>` would silently
    /// first-parent and hide the real diff.
    Merge {
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
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
        #[arg(
            value_name = "MERGE_SHA",
            long_help = "Merge commit SHA (7 to 40 hex characters; case-insensitive)"
        )]
        merge_sha: CommitShaPrefix,
    },
    /// Check a GitHub PR. The diff comes from `gh pr diff`.
    Pr {
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
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
        /// PR URL like `https://github.com/owner/repo/pull/123`.
        #[arg(value_name = "PR_URL")]
        pr_url: String,
    },
    /// Evaluate ONE commit against multiple series. Produces a per-series
    /// summary row plus a worst-case verdict across all series. Merge
    /// SHAs evaluate as the first-parent diff, like `check merge`.
    Multi {
        /// Series to evaluate against. Repeat the flag or use a
        /// comma-separated list.
        #[arg(long, required = true, value_delimiter = ',')]
        series: Vec<SeriesName>,
        #[arg(long, default_value = ".")]
        repo_dir_path: PathBuf,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
        #[arg(
            value_name = "COMMIT_SHA",
            long_help = "Commit SHA (7 to 40 hex characters; case-insensitive)"
        )]
        commit: CommitShaPrefix,
    },
    /// Evaluate many commits against one or more series. One row per
    /// (commit, series) pair. Blank lines and `#` comments in the
    /// commits file are skipped. Merge SHAs evaluate as the
    /// first-parent diff, like `check merge`, with `pr_commits` and
    /// `parent_count` on the row.
    Batch {
        /// Series to evaluate against. Repeat the flag or use a
        /// comma-separated list.
        #[arg(long, required = true, value_delimiter = ',')]
        series: Vec<SeriesName>,
        #[arg(long, default_value = ".")]
        repo_dir_path: PathBuf,
        /// File of one commit SHA prefix per line (7 to 40 hex characters;
        /// trailing `# annotation` ignored); `-` reads stdin.
        #[arg(long, required = true)]
        commits_file_path: PathBuf,
        #[command(flatten)]
        source: SourcePinArgs,
        #[command(flatten)]
        target: TargetRepoArgs,
        #[command(flatten)]
        diagnostics: CheckFlags,
    },
}
