// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::Subcommand;

use backhopper_core::model::names::{Mfa, ModuleName, ProjectName, SeriesName, TagName};

#[derive(Debug, Subcommand)]
pub enum SnapshotsCmd {
    /// List tags that have no snapshot on disk yet (read-only).
    #[command(name = "list_tags")]
    ListTags {
        #[arg(long)]
        project: Option<ProjectName>,
    },
    /// Generate snapshots for tags that don't have one yet.
    Generate {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, help = "Skip ls-remote freshness check")]
        no_remote_check: bool,
        #[arg(long, help = "Plan only; do not write to disk")]
        dry_run: bool,
        /// Only consider tags at or after this one (version-sorted).
        #[arg(long, conflicts_with = "series")]
        since: Option<TagName>,
        /// Fan out across every dep pinned by the named series. Skips
        /// self-pins.
        #[arg(long, conflicts_with_all = ["project", "since"])]
        series: Option<SeriesName>,
    },
    /// List existing snapshots for a project.
    List {
        #[arg(long)]
        project: ProjectName,
    },
    /// Print a snapshot's canonical text.
    Show {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        /// Print only the named module's section.
        #[arg(long)]
        module: Option<ModuleName>,
    },
    /// Verify a snapshot's canonical-form invariants.
    Verify {
        #[arg(long, conflicts_with_all = ["all", "coverage"])]
        project: Option<ProjectName>,
        #[arg(long, requires = "project", conflicts_with_all = ["all", "coverage"])]
        tag: Option<TagName>,
        /// Walk every stored snapshot and parse-verify each one.
        #[arg(long, conflicts_with_all = ["project", "tag", "coverage"])]
        all: bool,
        /// Report every `[[series]]` pin missing from the snapshot store.
        #[arg(long, conflicts_with_all = ["project", "tag", "all"])]
        coverage: bool,
    },
    /// Rebuild a snapshot from source (replace the on-disk copy).
    Rebuild {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long, help = "Plan only; do not write to disk")]
        dry_run: bool,
    },
    /// Re-emit every stored snapshot at the current format version.
    /// Reads each file at its declared `format-version`, writes it back
    /// at the running binary's `FORMAT_VERSION`. Re-runs the extractor
    /// only if explicitly asked.
    Migrate {
        /// Migrate only this project's snapshots.
        #[arg(long)]
        project: Option<ProjectName>,
        /// Plan only; do not write.
        #[arg(long)]
        dry_run: bool,
    },
    /// Look up one or more MFAs against a single snapshot.
    Lookup {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long, action = clap::ArgAction::Append, required = true)]
        mfa: Vec<Mfa>,
        #[arg(long)]
        include_hidden: bool,
    },
    /// Report the first and last tag at which each MFA appears, with the
    /// snapshot-anchored commit SHA at each endpoint.
    Introduced {
        #[arg(long)]
        project: ProjectName,
        #[arg(long, action = clap::ArgAction::Append, required = true)]
        mfa: Vec<Mfa>,
        #[arg(long)]
        include_hidden: bool,
        /// Emit a per-tag presence row for every stored tag, not just the endpoints.
        #[arg(long)]
        timeline: bool,
    },
    /// List modules a snapshot covers.
    Modules {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long)]
        include_hidden: bool,
    },
    /// List a module's exports in a snapshot.
    Exports {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long)]
        module: String,
    },
    /// Diff two snapshots of the same project, or two whole series.
    Diff {
        #[arg(long, requires_all = ["from", "to"], conflicts_with_all = ["from_series", "to_series"])]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        from: Option<TagName>,
        #[arg(long, requires = "project")]
        to: Option<TagName>,
        /// Series whose pins act as the `from` side.
        #[arg(long, requires = "to_series")]
        from_series: Option<SeriesName>,
        /// Series whose pins act as the `to` side.
        #[arg(long, requires = "from_series")]
        to_series: Option<SeriesName>,
    },
}
