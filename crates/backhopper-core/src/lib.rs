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
pub mod snapshot;
pub mod store;
pub mod suites;

pub use errors::{Error, Result};
pub use model::names::{
    ApplicationName, Arity, AttributeName, BehaviourName, CallbackName, CommitSha, FieldName,
    FunctionName, Mfa, ModuleName, ProjectName, RecordName, SeriesName, TagName, TypeName,
};
pub use model::pin::Pin;
pub use model::snapshot::{Snapshot, SnapshotHeader, state};
pub use model::symbol::{Reference, SymbolKind, SymbolRef};
pub use model::verdict::{Reason, SeriesVerdict, Verdict};