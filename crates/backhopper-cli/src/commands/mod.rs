// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Command dispatch.

pub mod auto_generate;
pub mod availability;
pub mod batch_plan;
pub mod bisect;
pub mod cache;
pub mod check;
pub mod config;
pub mod context;
pub mod doctor;
pub mod init;
pub mod macro_env;
pub mod pin_bump;
pub mod pin_coverage;
pub mod projects;
pub mod rabbitmq_components;
pub mod rev;
pub mod schema;
pub mod self_snapshot;
pub mod series;
pub mod sha_prefix;
pub mod shell;
pub mod siblings;
pub mod snapshot_cache;
pub mod snapshots;
pub mod suggest;
pub mod suites;
pub mod summary;
pub mod target_repo;
pub mod tree_source;
pub mod verdict_cache;
pub mod version;
pub mod xref;
pub mod xref_backport_applicability;

use crate::cli::{Cli, Group};
use crate::errors::CliResult;

pub fn dispatch(cli: Cli) -> CliResult<i32> {
    match cli.group {
        Group::Doctor(cmd) => doctor::handle(&cli.global, cmd),
        Group::Init(cmd) => init::handle(&cli.global, cmd),
        Group::Projects { cmd } => projects::handle(&cli.global, cmd),
        Group::Series { cmd } => series::handle(&cli.global, cmd),
        Group::Snapshots { cmd } => snapshots::handle(&cli.global, cmd),
        Group::Check { cmd } => check::handle(&cli.global, cmd),
        Group::Bisect { cmd } => bisect::handle(&cli.global, cmd),
        Group::Cache { cmd } => cache::handle(&cli.global, cmd),
        Group::Config { cmd } => config::handle(&cli.global, cmd),
        Group::Shell { cmd } => shell::handle(&cli.global, cmd),
        Group::Xref { cmd } => xref::handle(&cli.global, cmd),
        Group::Suites { cmd } => suites::handle(&cli.global, cmd),
        Group::Schema { cmd } => schema::handle(&cli.global, cmd),
        Group::Rev { cmd } => rev::handle(&cli.global, cmd),
        Group::Siblings { cmd } => siblings::handle(&cli.global, cmd),
        Group::Version => version::handle(&cli.global),
    }
}
