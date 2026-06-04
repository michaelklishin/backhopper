// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Core types, snapshot I/O, and compatibility analysis for `backhopper`.

pub mod app_src;
pub mod compat;
pub mod config;
pub mod erlang_macros;
pub mod errors;
pub mod git;
pub mod model;
#[cfg(feature = "schemars")]
pub mod schema;
pub mod schema_diff;
pub mod snapshot;
pub mod store;
pub mod suites;

pub use errors::{Error, Result};
pub use model::batch::{BatchPayload, BatchQuery, BatchResult, PinPayload};
pub use model::names::{
    ApplicationName, Arity, AttributeName, BehaviourName, CallbackName, CommitSha, FieldName,
    FunctionName, Mfa, ModuleName, ProjectName, RecordName, RelativePath, SeriesName, TagGlob,
    TagName, TypeName,
};
pub use model::pin::{Pin, PinSelect, PinSpec};
pub use model::pr_commit::{PrCommit, PrCommitKind};
pub use model::snapshot::{Snapshot, SnapshotHeader, state};
pub use model::summary::{SummaryRow, VerdictKind};
pub use model::symbol::{Reference, SymbolKind, SymbolRef};
pub use model::verdict::{Reason, SeriesVerdict, Verdict};
