// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Command dispatch.

pub mod check;
pub mod config;
pub mod context;
pub mod projects;
pub mod rabbitmq;
pub mod rabbitmq_components;
pub mod series;
pub mod shell;
pub mod snapshots;
pub mod suites;
pub mod tree_source;
pub mod version;
pub mod xref;

use crate::cli::{Cli, Group};
use crate::errors::CliResult;

pub fn dispatch(cli: Cli) -> CliResult<i32> {
    match cli.group {
        Group::Projects { cmd } => projects::handle(&cli.global, cmd),
        Group::Series { cmd } => series::handle(&cli.global, cmd),
        Group::Snapshots { cmd } => snapshots::handle(&cli.global, cmd),
        Group::Check { cmd } => check::handle(&cli.global, cmd),
        Group::Config { cmd } => config::handle(&cli.global, cmd),
        Group::Shell { cmd } => shell::handle(&cli.global, cmd),
        Group::Xref { cmd } => xref::handle(&cli.global, cmd),
        Group::Suites { cmd } => suites::handle(&cli.global, cmd),
        Group::Rabbitmq { cmd } => rabbitmq::handle(&cli.global, cmd),
        Group::Version => version::handle(&cli.global),
    }
}