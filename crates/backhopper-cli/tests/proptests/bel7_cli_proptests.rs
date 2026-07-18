// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Property tests for the `bel7-cli` integration surfaces in the CLI:
//! every `TableStyle` parses round-trip; every `CompletionShell` value
//! round-trips through the top-level argv. The index is taken
//! modulo the live variant count so a new variant in `bel7-cli`
//! does not silently drop out of coverage.

use std::str::FromStr;

use bel7_cli::{CompletionShell, TableStyle};
use clap::{Parser, ValueEnum};
use proptest::prelude::*;

use backhopper_cli::Cli;

fn all_table_style_names() -> Vec<String> {
    TableStyle::value_variants()
        .iter()
        .map(|s| s.to_possible_value().unwrap().get_name().to_owned())
        .collect()
}

fn all_completion_shell_names() -> Vec<String> {
    CompletionShell::value_variants()
        .iter()
        .map(|s| s.to_possible_value().unwrap().get_name().to_owned())
        .collect()
}

proptest! {
    #[test]
    fn every_table_style_name_round_trips_through_argv(idx in 0usize..64) {
        let names = all_table_style_names();
        let name = &names[idx % names.len()];
        let cli = Cli::try_parse_from([
            "backhopper",
            "--table-style",
            name.as_str(),
            "version",
        ]).expect("should parse a valid table style");
        let echoed = cli
            .global
            .table_style
            .to_possible_value()
            .unwrap()
            .get_name()
            .to_owned();
        prop_assert_eq!(&echoed, name);
    }

    #[test]
    fn every_completion_shell_value_parses_in_completions_subcommand(idx in 0usize..64) {
        let names = all_completion_shell_names();
        let name = &names[idx % names.len()];
        Cli::try_parse_from(["backhopper", "shell", "completions", name.as_str()])
            .expect("every advertised shell must parse");
    }

    #[test]
    fn completion_shell_from_str_accepts_lowercase_and_aliases(
        prefix in "nu|pwsh|bash|zsh|fish|elvish|nushell|powershell"
    ) {
        <CompletionShell as FromStr>::from_str(&prefix)
            .expect("documented spellings should parse");
    }

    #[test]
    fn parsing_a_table_style_round_trips_through_display(idx in 0usize..64) {
        let names = all_table_style_names();
        let name = &names[idx % names.len()];
        let style = TableStyle::from_str(name, false)
            .expect("clap-advertised name should parse");
        let echoed = style.to_possible_value().unwrap().get_name().to_owned();
        prop_assert_eq!(&echoed, name);
    }
}
