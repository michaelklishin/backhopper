// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Re-exports of the value types the driver passes through from
//! `backhopper-core`.
//!
//! The driver does not redefine verdict or reason types: anything
//! that lives in `backhopper_core::model` flows through unchanged so
//! callers can match on the same enums the CLI itself produces.

pub use backhopper_core::model::batch::{BatchPayload, BatchQuery, BatchResult, PinPayload};
pub use backhopper_core::model::cache::{
    CacheLevel, CacheLevelStats, CacheListPayload, CacheListRow, CacheMutationPayload,
    CacheShowPayload, CacheStatsPayload,
};
pub use backhopper_core::model::clearance::{
    BumpSummary, ClearanceFacts, ReasonHistogram, RoundClearance,
};
pub use backhopper_core::model::names::{
    ApplicationName, Arity, AttributeName, BehaviourName, CallbackName, CommitSha, CommitShaPrefix,
    DependencyName, DependencyVersion, FieldName, FunctionName, GitRef, MacroName, Mfa, ModuleName,
    ProjectName, RecordName, SeriesName, TagGlob, TagName, TypeName,
};
pub use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};
pub use backhopper_core::model::pr_commit::{PrCommit, PrCommitKind};
pub use backhopper_core::model::sibling_drift::{
    CandidateFeatures, Confidence, ScoreComponents, Scored, SiblingCandidate, SiblingDoctorReport,
    SiblingDriftAction, SinceDerivation, VocabularySource,
};
pub use backhopper_core::model::snapshot::{
    FORMAT_VERSION, HrlFile, Module, Provenance, SUPPORTED_FORMAT_VERSIONS, Snapshot,
    SnapshotHeader, VendoredDep, VendoredDepSource, VersionedMachineVersion, WireConstantBinding,
    WireValue, state as snapshot_state,
};
pub use backhopper_core::model::snapshot_diff::{
    CrossSeriesDiffPayload, DiffPayload, QualifiedFunArity, QualifiedRecord, QualifiedTypeArity,
    VersionedMachineVersionChange, WireConstantChange,
};
pub use backhopper_core::model::summary::{SummaryRow, VerdictKind};
pub use backhopper_core::model::symbol::{RefOrigin, Reference, SymbolKind, SymbolRef};
pub use backhopper_core::model::verdict::{
    BumpStatus, Diagnostics, FileKind, InapplicableReason, NeedsPin, PatchFacts, PinBump,
    PinVerdict, Reason, SeriesSummary, SeriesVerdict, SnapshotSide, SourceDelta, TouchedKinds,
    Verdict,
};
pub use backhopper_core::suites::{
    SuiteInclusionReason, SuitePlan, SuitePlanEntry, SuiteRef, UncoveredApplication,
};
