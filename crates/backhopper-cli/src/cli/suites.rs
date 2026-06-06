// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use backhopper_core::model::names::{ApplicationName, Mfa, ModuleName};

use crate::cli::tree_source::TreeSource;

#[derive(Debug, Subcommand)]
#[allow(clippy::enum_variant_names)]
pub enum SuitesCmd {
    /// Suites that reference any of the supplied modules.
    #[command(name = "list_for_modules")]
    ListForModules {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long, value_delimiter = ',', required = true)]
        module: Vec<ModuleName>,
    },
    /// Suites that call any of the supplied MFAs.
    #[command(name = "list_for_mfas")]
    ListForMfas {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long, value_delimiter = ',', required = true)]
        mfa: Vec<Mfa>,
    },
    /// All callers of an MFA, without the suite filter.
    #[command(name = "list_callers_of")]
    ListCallersOf {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        mfa: Mfa,
    },
    /// Plan which suites to run for a given set of modified files.
    Plan(SuitesPlanArgs),
}

#[derive(Debug, Args)]
pub struct SuitesPlanArgs {
    /// Repo root directory: the tree backhopper will scan for `.app.src`
    /// files and `_SUITE.erl` files.
    #[arg(long)]
    pub repo_dir_path: PathBuf,

    /// File holding a newline-separated list of modified paths. Pass
    /// `-` to read from stdin. Mutually exclusive with `--modified-path`.
    #[arg(long, conflicts_with = "modified_path")]
    pub modified_paths_file_path: Option<PathBuf>,

    /// One modified path. Repeatable; mutually exclusive with
    /// `--modified-paths-file-path`.
    #[arg(long)]
    pub modified_path: Vec<PathBuf>,

    /// Library applications: modules under these apps trigger
    /// cross-application suite inclusion. Comma-separated.
    #[arg(long, value_delimiter = ',')]
    pub library_app: Vec<ApplicationName>,

    /// Auto-derive the library-applications set from `.app.src`
    /// `applications` fields: any in-tree app depended on by 2+
    /// other apps is treated as a library. Combines with explicit
    /// `--library-app` entries.
    #[arg(long)]
    pub auto_library_apps: bool,
}
