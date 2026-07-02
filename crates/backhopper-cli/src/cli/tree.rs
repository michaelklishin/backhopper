// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

use backhopper_core::model::names::ModuleName;

#[derive(Debug, Subcommand)]
pub enum TreeCmd {
    /// Print one module's API surface from a repo ref, in the
    /// canonical snapshot format. Two invocations with two `--ref`
    /// values answer "which branch has what".
    Show {
        #[arg(
            long,
            default_value = ".",
            env = "BACKHOPPER_REPO_DIR_PATH",
            help = "Repo whose tree to read"
        )]
        repo_dir_path: PathBuf,
        #[arg(long, default_value = "HEAD", help = "Ref to read the tree at")]
        r#ref: String,
        #[arg(
            long,
            value_enum,
            help = "Print one entry class instead of the whole module"
        )]
        class: Option<EntryClass>,
        #[arg(value_name = "MODULE")]
        module: ModuleName,
    },
}

/// One snapshot entry class: the value set mirrors the format's line
/// keywords, not a new taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EntryClass {
    Exports,
    Specs,
    Callbacks,
    Records,
    Types,
    Behaviours,
    Deprecated,
}

impl EntryClass {
    /// The canonical format's entry keyword this class keeps.
    pub fn keyword(self) -> &'static str {
        match self {
            EntryClass::Exports => "export",
            EntryClass::Specs => "spec",
            EntryClass::Callbacks => "callback",
            EntryClass::Records => "record",
            EntryClass::Types => "type",
            EntryClass::Behaviours => "behaviour",
            EntryClass::Deprecated => "deprecated",
        }
    }
}
