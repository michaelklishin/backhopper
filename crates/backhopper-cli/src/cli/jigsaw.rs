// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{ArgAction, ArgGroup, Args, Subcommand};

use backhopper_core::model::names::{HostName, TagName};
use backhopper_core::model::release_version::{ErlangMajor, HostSeries};

#[derive(Debug, Subcommand)]
pub enum JigsawCmd {
    /// Work out which snapshots a run needs and which of them the store lacks.
    Plan(PlanArgs),
    /// Check plugin apps from directories against every patch release of a host series.
    Check(CheckArgs),
}

#[derive(Debug, Args)]
pub struct PlanArgs {
    #[arg(long, help = "The configured [[host]] to plan for")]
    pub host: HostName,

    #[arg(
        long,
        help = "Release series to plan, as MAJOR.MINOR (for example 4.2)"
    )]
    pub series: HostSeries,

    #[arg(
        long,
        help = "Newest release tag to include; the build patch (default: the newest of the series)"
    )]
    pub up_to: Option<TagName>,

    #[arg(
        long = "erlang-major",
        action = ArgAction::Append,
        help = "Erlang major to test; repeatable (default: the host's erlang_majors)"
    )]
    pub erlang_majors: Vec<ErlangMajor>,

    #[arg(
        long,
        help = "Host checkout to list tags from (default: the host project's git_url)"
    )]
    pub repo_dir_path: Option<PathBuf>,

    #[arg(
        long,
        help = "Also write the JSON envelope to this file, for `snapshots generate --from-jigsaw-plan`"
    )]
    pub plan_file_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("plan_source").required(true).args(["plan_file_path", "host"])))]
pub struct CheckArgs {
    #[arg(
        long,
        conflicts_with_all = ["series", "up_to", "erlang_majors"],
        help = "A plan written by `jigsaw plan --plan-file-path`"
    )]
    pub plan_file_path: Option<PathBuf>,

    #[arg(
        long,
        requires = "series",
        help = "The configured [[host]] to draft the plan for, from the snapshot store alone"
    )]
    pub host: Option<HostName>,

    #[arg(
        long,
        requires = "host",
        help = "Release series to check, as MAJOR.MINOR (for example 4.2)"
    )]
    pub series: Option<HostSeries>,

    #[arg(
        long,
        requires = "host",
        help = "Newest release tag to include; the build patch (default: the newest stored)"
    )]
    pub up_to: Option<TagName>,

    #[arg(
        long = "erlang-major",
        action = ArgAction::Append,
        requires = "host",
        help = "Erlang major to test; repeatable (default: the host's erlang_majors)"
    )]
    pub erlang_majors: Vec<ErlangMajor>,

    #[arg(
        long = "plugin-dir-path",
        action = ArgAction::Append,
        required = true,
        help = "A directory holding plugin apps; repeatable"
    )]
    pub plugin_dir_paths: Vec<PathBuf>,
}
