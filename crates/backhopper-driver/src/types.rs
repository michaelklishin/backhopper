// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Re-exports of the value types the driver passes through from
//! `backhopper-core`.
//!
//! The driver does not redefine verdict or reason types: anything
//! that lives in `backhopper_core::model` flows through unchanged so
//! callers can match on the same enums the CLI itself produces.

pub use backhopper_core::model::names::{
    ApplicationName, Arity, AttributeName, BehaviourName, CallbackName, CommitSha, CommitShaPrefix,
    DependencyName, DependencyVersion, FieldName, FunctionName, MacroName, Mfa, ModuleName,
    ProjectName, RecordName, SeriesName, TagGlob, TagName, TypeName,
};
pub use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};
pub use backhopper_core::model::snapshot::{
    FORMAT_VERSION, HrlFile, Module, Provenance, SUPPORTED_FORMAT_VERSIONS, Snapshot,
    SnapshotHeader, VendoredDep, VendoredDepSource, VersionedMachineVersion, WireConstantBinding,
    WireValue, state as snapshot_state,
};
pub use backhopper_core::model::symbol::{Reference, SymbolKind, SymbolRef};
pub use backhopper_core::model::verdict::{
    FileKind, InapplicableReason, PinVerdict, Reason, SeriesVerdict, SnapshotSide, TouchedKinds,
    Verdict,
};
