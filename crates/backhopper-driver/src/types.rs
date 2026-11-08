// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Re-exports of the value types the driver passes through from
//! `backhopper-core`.
//!
//! The driver does not redefine verdict or reason types: anything
//! defined in `backhopper_core::model` flows through unchanged so
//! callers can match on the same enums the CLI itself produces.

pub use backhopper_core::jigsaw::findings::{JigsawFindingKind, JigsawWarningKind, WithheldReason};
pub use backhopper_core::jigsaw::plugin_set::AppRole;
pub use backhopper_core::jigsaw::references::NotChecked;
pub use backhopper_core::jigsaw::report::JigsawCell;
pub use backhopper_core::jigsaw::{SnapshotGap, TagSourceKind};
pub use backhopper_core::model::app_manifest::{
    AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable,
};
pub use backhopper_core::model::apply::{
    ApplyForecast, ApplySummary, PathApplyOutcome, UnassessedReason,
};
pub use backhopper_core::model::batch::{BatchPayload, BatchQuery, BatchResult, PinPayload};
pub use backhopper_core::model::cache::{
    CacheLevel, CacheLevelStats, CacheListPayload, CacheListRow, CacheMutationPayload,
    CacheShowPayload, CacheStatsPayload,
};
pub use backhopper_core::model::clearance::{
    ApplyRollup, BumpSummary, ClearanceFacts, ReasonHistogram, RoundClearance, TargetFindingsRollup,
};
pub use backhopper_core::model::eval::{
    BuildOutcome, CorpusEntry, EvalReport, ForecastEntry, ForecastMiss, ForecastReport,
    ForecastedConflict, MissedBreak, ObservedApply, PredictedConflict, Ratio,
};
pub use backhopper_core::model::evaluation::AggregateVerdict;
pub use backhopper_core::model::findings::TargetFindings;
pub use backhopper_core::model::fingerprint::{FINGERPRINT_VERSION, VerdictFingerprint};
pub use backhopper_core::model::macro_decl::{MacroBody, MacroDecl, MacroKey};
pub use backhopper_core::model::names::{
    ApplicationName, Arity, BehaviourName, CommitSha, CommitShaPrefix, ConfKey, DependencyName,
    DependencyVersion, FieldName, FunctionName, GitRef, HostFactsDigest, HostName, MacroName,
    MakeVariableName, Mfa, ModuleName, PartialName, ProjectName, RecordName, RelativePath,
    SeriesName, TagGlob, TagName, TypeName, ValidatorName,
};
pub use backhopper_core::model::normalised::{NormalisedDefault, NormalisedMacroBody};
pub use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};
pub use backhopper_core::model::pr_commit::{PrCommit, PrCommitKind};
pub use backhopper_core::model::release_version::{ErlangMajor, HostSeries};
pub use backhopper_core::model::resolver_coverage::{ResolverClass, ResolverCoverage};
pub use backhopper_core::model::schema_files::{PartialFile, SchemaFile};
pub use backhopper_core::model::snapshot::{
    FORMAT_VERSION, HeaderRef, HostReleaseFacts, HrlFile, Module, Provenance, RecordDecl,
    RecordField, SUPPORTED_FORMAT_VERSIONS, Snapshot, SnapshotHeader, VendoredDep,
    VendoredDepSource, VersionedMachineVersion, WireConstantBinding, WireValue,
    state as snapshot_state,
};
pub use backhopper_core::model::snapshot_diff::{
    CrossSeriesDiffPayload, DiffPayload, QualifiedFunArity, QualifiedRecord, QualifiedTypeArity,
    VersionedMachineVersionChange, WireConstantChange,
};
pub use backhopper_core::model::summary::{SummaryRow, VerdictKind};
pub use backhopper_core::model::symbol::{RefOrigin, Reference, SymbolKind, SymbolRef};
pub use backhopper_core::model::verdict::{
    ApplyConflictKind, BumpStatus, Diagnostics, FileKind, InapplicableReason, IndirectCallForm,
    IndirectCallTally, NeedsPin, PatchFacts, PinBump, PinVerdict, Reason, SeriesSummary,
    SeriesVerdict, SnapshotSide, SourceDelta, TouchedKinds, Verdict,
};
pub use backhopper_core::suites::{
    BroadImpactModule, SuiteInclusionReason, SuitePlan, SuitePlanEntry, SuiteRef,
    UncoveredApplication,
};
