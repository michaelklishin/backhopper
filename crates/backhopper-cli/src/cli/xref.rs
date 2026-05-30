// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Subcommand;

use backhopper_core::model::names::{Mfa, ModuleName};

use crate::cli::tree_source::TreeSource;

#[derive(Debug, Subcommand)]
#[allow(clippy::enum_variant_names)]
#[command(rename_all = "snake_case")]
pub enum XrefCmd {
    /// List callers of an MFA. With --transitive, follow the call graph.
    ListCallers {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        mfa: Mfa,
        #[arg(long)]
        transitive: bool,
    },
    /// List callees of an MFA. With --transitive, follow the call graph.
    ListCallees {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        mfa: Mfa,
        #[arg(long)]
        transitive: bool,
    },
    /// List external calls whose target is not defined or built-in.
    ListUndefined {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List exports that nothing else in the tree calls.
    ListUnusedExports {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List local functions that nothing in the same module calls.
    ListUnusedLocals {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List external calls to functions marked -deprecated.
    ListDeprecatedCalls {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List recorded unresolved (variable-module or variable-function) call sites.
    ListUnresolved {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List modules that depend on a given module.
    ListModuleDeps {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        module: ModuleName,
        /// Show modules that the given module *uses* rather than depend on it.
        #[arg(long)]
        forward: bool,
    },
    /// List `-behaviour(B)` implementer modules for a behaviour.
    ListBehaviourUsers {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        behaviour: String,
    },
    /// List strongly connected component cycles in the module-call graph.
    ListModuleCycles {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// Join an unconditional-test-exports reference list against a
    /// target-branch snapshot's `test_only_exports`. Emits per-row
    /// `applies`, `applies-with-adaptation`, or `n_a` to drive
    /// cascade planning.
    BackportApplicability {
        /// Path to a TOML reference list with `[[entry]]` arrays
        /// carrying `file`, `variant`, and `functions = ["name/arity", ...]`.
        #[arg(long = "reference-file-path")]
        reference_file_path: PathBuf,
        /// Path to the canonical text snapshot of the target branch.
        /// Generate via `backhopper snapshots generate --branch ...`.
        #[arg(long = "snapshot-file-path")]
        snapshot_file_path: PathBuf,
    },
}
