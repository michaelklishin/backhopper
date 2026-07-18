// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Wire payloads of the `snapshots diff` verbs.
//!
//! Modeled here so the CLI emits and the driver parses one definition.
//! The name-bearing fields carry the same serde-transparent newtypes the
//! rest of core uses, so the serialized form is unchanged and a consumer
//! stops re-validating identifiers it round-trips. The composite
//! (`fun_arity`, `type_arity`), path (`header`), and value fields stay
//! `String`: no name newtype fits them.

use serde::{Deserialize, Serialize};

use crate::model::names::{ModuleName, ProjectName, SeriesName, TagName};

/// API delta between two tags of one project, oriented `from -> to`:
/// `*_added` are present at `to` and absent at `from`; `*_removed` are
/// present at `from` and absent at `to`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffPayload {
    pub project: ProjectName,
    /// The older tag: the baseline of the delta.
    pub from: TagName,
    /// The newer tag: the target of the delta.
    pub to: TagName,
    pub modules_added: Vec<ModuleName>,
    pub modules_removed: Vec<ModuleName>,
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

impl DiffPayload {
    /// True when the diff records no API change on any tracked axis.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules_added.is_empty()
            && self.modules_removed.is_empty()
            && self.exports_added.is_empty()
            && self.exports_removed.is_empty()
            && self.types_added.is_empty()
            && self.types_removed.is_empty()
            && self.callbacks_added.is_empty()
            && self.callbacks_removed.is_empty()
            && self.headers_added.is_empty()
            && self.headers_removed.is_empty()
            && self.records_added.is_empty()
            && self.records_removed.is_empty()
            && self.versioned_machine_version_changes.is_empty()
            && self.wire_constant_changes.is_empty()
    }

    /// Count of removed symbols: the breaking side of an API delta.
    #[must_use]
    pub fn breaking_removal_count(&self) -> usize {
        self.modules_removed.len()
            + self.exports_removed.len()
            + self.callbacks_removed.len()
            + self.types_removed.len()
            + self.records_removed.len()
            + self.headers_removed.len()
    }
}

/// A versioned-machine version that was added, removed, or moved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum VersionedMachineVersionChange {
    Missing {
        module: ModuleName,
        side: String,
    },
    Drift {
        module: ModuleName,
        from: Option<u64>,
        to: Option<u64>,
    },
}

/// A wire-constant macro that was added, removed, or moved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WireConstantChange {
    Missing {
        module: ModuleName,
        side: String,
        macros: Vec<String>,
    },
    Drift {
        module: ModuleName,
        macro_name: String,
        from: String,
        to: String,
    },
}

/// A function or callback named `module:fun/arity`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualifiedFunArity {
    pub module: ModuleName,
    pub fun_arity: String,
}

/// A type named `module:type/arity`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualifiedTypeArity {
    pub module: ModuleName,
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
    pub from_series: SeriesName,
    /// The target series.
    pub to_series: SeriesName,
    /// One entry per project that differs between the two series.
    pub projects: Vec<DiffPayload>,
}
