// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Parity between the driver's `Verb` table and the CLI's clap tree.
//!
//! The CLI owns the clap tree and takes the driver as a dev-dependency,
//! so this test can compare the two directly. It guards one direction:
//! every wrapped `Verb` must name a real CLI command, so a renamed or
//! removed command surfaces here instead of at runtime. The reverse
//! (CLI commands with no wrapped verb) is intentional and left
//! undriven until a consumer asks for it.

use std::collections::BTreeSet;

use backhopper_cli::Cli;
use backhopper_driver::Verb;
use clap::CommandFactory;

/// Every leaf command's argv path below the binary name.
fn leaf_paths(cmd: &clap::Command, prefix: Vec<String>, out: &mut Vec<Vec<String>>) {
    if cmd.get_subcommands().next().is_none() {
        out.push(prefix);
        return;
    }
    for sub in cmd.get_subcommands() {
        let mut child = prefix.clone();
        child.push(sub.get_name().to_owned());
        leaf_paths(sub, child, out);
    }
}

fn cli_leaf_commands() -> BTreeSet<Vec<String>> {
    let root = Cli::command();
    let mut leaves = Vec::new();
    for sub in root.get_subcommands() {
        leaf_paths(sub, vec![sub.get_name().to_owned()], &mut leaves);
    }
    leaves.into_iter().collect()
}

#[test]
fn every_known_verb_maps_to_a_real_cli_command() {
    let cli = cli_leaf_commands();
    for verb in Verb::iter() {
        let path: Vec<String> = verb.cli_path().iter().map(|s| (*s).to_owned()).collect();
        assert!(
            cli.contains(&path),
            "Verb::{verb:?} maps to CLI path {path:?}, which is not a real command; \
             the driver's verb table drifted from the clap tree"
        );
    }
}

#[test]
fn some_cli_commands_are_intentionally_unwrapped() {
    let wrapped: BTreeSet<Vec<String>> = Verb::iter()
        .map(|v| v.cli_path().iter().map(|s| (*s).to_owned()).collect())
        .collect();
    // clap synthesises a `help` leaf under every group; not a verb
    let uncovered = cli_leaf_commands()
        .into_iter()
        .filter(|p| !wrapped.contains(p))
        .filter(|p| p.last().map(String::as_str) != Some("help"))
        .count();
    // the driver wraps only the verbs a consumer drives, so a gap is
    // expected; this pins that the two are not accidentally in lockstep
    assert!(uncovered > 0);
}
