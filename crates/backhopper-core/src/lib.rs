//! Core types, snapshot I/O, and compatibility analysis for `backhopper`.

pub mod compat;
pub mod config;
pub mod errors;
pub mod git;
pub mod model;
pub mod rabbitmq;
pub mod snapshot;
pub mod store;

pub use errors::{Error, Result};
pub use model::names::{
    Arity, CommitSha, FieldName, FunctionName, Mfa, ModuleName, ProjectName, RecordName,
    SeriesName, TagName, TypeName,
};
pub use model::pin::Pin;
pub use model::snapshot::{Snapshot, SnapshotHeader, state};
pub use model::symbol::{Reference, SymbolKind, SymbolRef};
pub use model::verdict::{Reason, SeriesVerdict, Verdict};
