// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Wire payloads of the `snapshots diff` verbs.
//!
//! Modeled here so the CLI emits and the driver parses one definition.
//! Field types are the stringly wire shape the CLI already ships;
//! newtyping them is a follow-up that keeps the same serialized form.

use serde::{Deserialize, Serialize};

/// API delta between two tags of one project, oriented `from -> to`:
/// `*_added` are present at `to` and absent at `from`; `*_removed` are
/// present at `from` and absent at `to`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffPayload {
    pub project: String,
    /// The older tag: the baseline of the delta.
    pub from: String,
    /// The newer tag: the target of the delta.
    pub to: String,
    pub modules_added: Vec<String>,
    pub modules_removed: Vec<String>,
    pub exports_added: Vec<QualifiedFunArity>,
    pub exports_removed: Vec<QualifiedFunArity>,
    pub types_added: Vec<QualifiedTypeArity>,
    pub types_removed: Vec<QualifiedTypeArity>,
    pub callbacks_added: Vec<QualifiedFunArity>,
    pub callbacks_removed: Vec<QualifiedFunArity>,
    pub headers_added: Vec<String>,
    pub headers_removed: Vec<String>,
    pub records_added: Vec<QualifiedRecord>,
    pub records_removed: Vec<QualifiedRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versioned_machine_version_changes: Vec<VersionedMachineVersionChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wire_constant_changes: Vec<WireConstantChange>,
}

/// A versioned-machine version that was added, removed, or moved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum VersionedMachineVersionChange {
    Missing {
        module: String,
        side: String,
    },
    Drift {
        module: String,
        from: Option<u64>,
        to: Option<u64>,
    },
}

/// A wire-constant macro that was added, removed, or moved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WireConstantChange {
    Missing {
        module: String,
        side: String,
        macros: Vec<String>,
    },
    Drift {
        module: String,
        macro_name: String,
        from: String,
        to: String,
    },
}

/// A function or callback named `module:fun/arity`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualifiedFunArity {
    pub module: String,
    pub fun_arity: String,
}

/// A type named `module:type/arity`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualifiedTypeArity {
    pub module: String,
    pub type_arity: String,
}

/// A record declared in a header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualifiedRecord {
    pub header: String,
    pub record: String,
}

/// Per-project diffs between the pins of two series, each oriented
/// `from_series -> to_series` (see [`DiffPayload`] for the per-project
/// `added`/`removed` orientation).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossSeriesDiffPayload {
    /// The baseline series.
    pub from_series: String,
    /// The target series.
    pub to_series: String,
    /// One entry per project that differs between the two series.
    pub projects: Vec<DiffPayload>,
}
