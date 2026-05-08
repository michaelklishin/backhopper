//! Command dispatch.

pub mod api;
pub mod compatibility;
pub mod completions;
pub mod config;
pub mod context;
pub mod projects;
pub mod series;
pub mod snapshots;
pub mod version;

use crate::cli::{Cli, Group};
use crate::errors::CliResult;

pub fn dispatch(cli: Cli) -> CliResult<i32> {
    match cli.group {
        Group::Projects { cmd } => projects::handle(&cli.global, cmd),
        Group::Series { cmd } => series::handle(&cli.global, cmd),
        Group::Snapshots { cmd } => snapshots::handle(&cli.global, cmd),
        Group::Api { cmd } => api::handle(&cli.global, cmd),
        Group::Compatibility { cmd } => compatibility::handle(&cli.global, cmd),
        Group::Config { cmd } => config::handle(&cli.global, cmd),
        Group::Completions { cmd } => completions::handle(&cli.global, cmd),
        Group::Version => version::handle(&cli.global),
    }
}
