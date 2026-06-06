// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Subcommand;

use backhopper_core::model::names::{Mfa, ModuleName};

use crate::cli::tree_source::TreeSource;

#[derive(Debug, Subcommand)]
#[allow(clippy::enum_variant_names)]
pub enum XrefCmd {
    /// List callers of an MFA. With --transitive, follow the call graph.
    #[command(name = "list_callers")]
    ListCallers {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        mfa: Mfa,
        #[arg(long)]
        transitive: bool,
    },
    /// List callees of an MFA. With --transitive, follow the call graph.
    #[command(name = "list_callees")]
    ListCallees {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        mfa: Mfa,
        #[arg(long)]
        transitive: bool,
    },
    /// List external calls whose target is not defined or built-in.
    #[command(name = "list_undefined")]
    ListUndefined {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List exports that nothing else in the tree calls.
    #[command(name = "list_unused_exports")]
    ListUnusedExports {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List local functions that nothing in the same module calls.
    #[command(name = "list_unused_locals")]
    ListUnusedLocals {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List external calls to functions marked -deprecated.
    #[command(name = "list_deprecated_calls")]
    ListDeprecatedCalls {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List recorded unresolved (variable-module or variable-function) call sites.
    #[command(name = "list_unresolved")]
    ListUnresolved {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// List modules that depend on a given module.
    #[command(name = "list_module_deps")]
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
    #[command(name = "list_behaviour_users")]
    ListBehaviourUsers {
        #[command(flatten)]
        tree: TreeSource,
        #[arg(long)]
        behaviour: String,
    },
    /// List strongly connected component cycles in the module-call graph.
    #[command(name = "list_module_cycles")]
    ListModuleCycles {
        #[command(flatten)]
        tree: TreeSource,
    },
    /// Join an unconditional-test-exports reference list against a
    /// target-branch snapshot's `test_only_exports`. Emits per-row
    /// `applies`, `applies-with-adaptation`, or `n_a` to drive
    /// cascade planning.
    #[command(name = "backport_applicability")]
    BackportApplicability {
        /// Path to a TOML reference list with `[[entry]]` arrays
        /// carrying `file`, `variant`, and `functions = ["name/arity", ...]`.
        #[arg(long)]
        reference_file_path: PathBuf,
        /// Path to the canonical text snapshot of the target branch.
        /// Generate via `backhopper snapshots generate --branch ...`.
        #[arg(long)]
        snapshot_file_path: PathBuf,
    },
}
