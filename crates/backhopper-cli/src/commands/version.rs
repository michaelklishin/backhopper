// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use crate::outcome::CommandOutcome;
use clap::{CommandFactory, crate_version};
use serde::Serialize;

use crate::cli::{Cli, GlobalArgs};
use crate::errors::CliResult;
use crate::output::{OutputContext, render};

#[derive(Debug, Serialize)]
struct VersionPayload<'a> {
    name: &'a str,
    version: &'a str,
    /// Capability list: every verb this binary ships, as
    /// space-separated paths (`"check batch"`, `"suites plan"`).
    /// Lets the driver probe verb presence by name instead of
    /// inferring it from a schema version number.
    verbs: Vec<String>,
}

pub fn handle(args: &GlobalArgs) -> CliResult<CommandOutcome> {
    let payload = VersionPayload {
        name: "backhopper",
        version: crate_version!(),
        verbs: collect_verbs(),
    };
    let ctx = OutputContext::new(args.formatter, "version");
    render(&ctx, &payload, |w| {
        writeln!(w, "{} {}", payload.name, payload.version)?;
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

/// Walk the clap command tree so the list can never drift from the
/// parser: a leaf group is one verb, a group with subcommands is one
/// verb per subcommand.
fn collect_verbs() -> Vec<String> {
    let cmd = Cli::command();
    let mut out: Vec<String> = Vec::new();
    for group in cmd.get_subcommands() {
        let group_name = group.get_name();
        if group_name == "help" {
            continue;
        }
        let mut had_sub = false;
        for sub in group.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }
            had_sub = true;
            out.push(format!("{group_name} {}", sub.get_name()));
        }
        if !had_sub {
            out.push(group_name.to_owned());
        }
    }
    out.sort();
    out
}
