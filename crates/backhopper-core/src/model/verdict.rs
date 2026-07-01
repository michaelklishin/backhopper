// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::compat::arg_shape::ArgShape;
use crate::model::names::{
    Arity, CommitSha, DependencyName, FieldName, FunctionName, GitRef, MacroName, ModuleName,
    ProjectName, RecordName, RelativePath, TagName, TypeName,
};
use crate::model::pin::Pin;
use crate::model::pr_commit::PrCommit;
use crate::model::resolver_coverage::ResolverClass;
use crate::model::symbol::{SymbolKind, SymbolRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Compatible,
    RequiresAdaptation { reasons: Vec<Reason> },
    Incompatible { reasons: Vec<Reason> },
    Inapplicable { reason: InapplicableReason },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "reason", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum InapplicableReason {
    NoErlangSurfaceTouched,
    OnlyDocsTouched,
    OnlyTestFixturesTouched,
    OnlySchemaTouched,
    OnlyMakefileTouched,
    OnlyMixExsTouched,
    OnlyCiWorkflowTouched,
    OnlyAppSrcTouched,
    OnlyRebarConfigTouched,
    /// Every changed `.erl` hunk is a Variant A unwrap: dropping
    /// `-ifdef(TEST).` or `-endif.` around an `-export(...)` directive,
    /// or rewriting `-compile(export_all).` to an explicit
    /// `-export([...])`. No call sites, no specs, no bodies touched —
    /// only test-visibility metadata.
    OnlyTestVisibilityChanged,
    /// The patch touches Erlang surface but every referenced symbol
    /// resolves to the self-pin or to a local definition: every
    /// tracked-dep pin sees zero in-scope references. Without this
    /// reason the operator would see a wall of vacuous `Compatible`
    /// rows for a patch that backhopper genuinely has no signal on.
    OnlySelfSurfaceTouched,
    /// Every touched path lives in a sibling project's scope, not this
    /// pin's. The pin has nothing to say about the patch. `project`
    /// names the first sibling project (alphabetical) that owns the
    /// out-of-scope paths.
    OutOfScopeFor {
        project: ProjectName,
    },
    /// Every touched path is unclaimed by any configured project's
    /// scan_paths or app_roots. Vendored files, docs, build scripts.
    Untracked,
    /// `--target-repo-dir-path` was supplied and every touched path
    /// is missing from the target tree, with no configured
    /// translation mapping any of them. The patch as written does
    /// not apply to the target.
    PathsMissingOnTarget {
        paths: Vec<RelativePath>,
    },
}

impl InapplicableReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoErlangSurfaceTouched => "no_erlang_surface_touched",
            Self::OnlyDocsTouched => "only_docs_touched",
            Self::OnlyTestFixturesTouched => "only_test_fixtures_touched",
            Self::OnlySchemaTouched => "only_schema_touched",
            Self::OnlyMakefileTouched => "only_makefile_touched",
            Self::OnlyMixExsTouched => "only_mix_exs_touched",
            Self::OnlyCiWorkflowTouched => "only_ci_workflow_touched",
            Self::OnlyAppSrcTouched => "only_app_src_touched",
            Self::OnlyRebarConfigTouched => "only_rebar_config_touched",
            Self::OnlyTestVisibilityChanged => "only_test_visibility_changed",
            Self::OnlySelfSurfaceTouched => "only_self_surface_touched",
            Self::OutOfScopeFor { .. } => "out_of_scope_for",
            Self::Untracked => "untracked",
            Self::PathsMissingOnTarget { .. } => "paths_missing_on_target",
        }
    }
}

impl Verdict {
    pub fn from_reasons(reasons: Vec<Reason>) -> Self {
        if reasons.is_empty() {
            return Self::Compatible;
        }
        if reasons.iter().any(Reason::is_blocking) {
            Self::Incompatible { reasons }
        } else {
            Self::RequiresAdaptation { reasons }
        }
    }

    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }

    pub fn reasons(&self) -> &[Reason] {
        match self {
            Self::Compatible | Self::Inapplicable { .. } => &[],
            Self::RequiresAdaptation { reasons } | Self::Incompatible { reasons } => reasons,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    MissingSymbol {
        symbol: SymbolRef,
        /// Earliest snapshot tag strictly later than the pin where the
        /// symbol appears. `Some` reclassifies the reason as
        /// adaptable: land the dep pin bump first.
        first_seen_at_tag: Option<TagName>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        needs_pin_at_least: Option<NeedsPin>,
        suggested_replacement: Option<SymbolRef>,
    },
    ArityChanged {
        module: ModuleName,
        function: FunctionName,
        expected: Arity,
        found: Vec<Arity>,
        /// Earliest snapshot tag strictly later than the pin exporting
        /// the expected arity. Same bump-first semantics as
        /// `MissingSymbol::first_seen_at_tag`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_available_at: Option<TagName>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        needs_pin_at_least: Option<NeedsPin>,
    },
    SignatureChanged {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        expected_spec: String,
        found_spec: String,
    },
    FileAbsent {
        path: PathBuf,
    },
    ContextDrift {
        path: PathBuf,
        hunk_index: usize,
    },
    /// The contiguous preimage block (Context and Removed lines from
    /// the hunk) was found verbatim at a different starting line.
    /// Cherry-pick adapters handle this with the `-x` trailer; the
    /// operator just needs the delta to confirm the patch's intent
    /// still applies.
    PreimageDrifted {
        path: PathBuf,
        hunk_index: usize,
        line_delta: isize,
    },
    /// The hunk's preimage matches, but its added lines land at a file
    /// edge where the target already has divergent content the base
    /// lacked: an add/add a contiguous preimage match cannot see. The
    /// 3-way cherry-pick conflicts where the preimage-only check reads
    /// clean. Non-blocking: the operator resolves the overlap.
    PostimageCollision {
        path: PathBuf,
        hunk_index: usize,
    },
    /// The preimage block does not appear anywhere in the file. The
    /// patch was authored against a tree where the lines existed; on
    /// this pin they don't. Structurally inapplicable: the operator
    /// needs an excerpt to reason about why.
    PreimageMissing {
        path: PathBuf,
        hunk_index: usize,
        preimage_excerpt: String,
    },
    DeprecatedUsage {
        symbol: SymbolRef,
        since: Option<TagName>,
        replacement: Option<SymbolRef>,
    },
    NowHidden {
        module: ModuleName,
    },
    RecordFieldsChanged {
        record: RecordName,
        expected: Vec<FieldName>,
        found: Vec<FieldName>,
    },
    UnsupportedFileType {
        path: PathBuf,
    },
    UntrackedModuleMissing {
        module: ModuleName,
    },
    /// Call-site argument shapes don't satisfy any clause head at the
    /// pin. Emitted when both the call and the pin's clause patterns
    /// are concrete enough to compare; `Unknown` on either side is the
    /// escape hatch that suppresses the reason.
    ClauseMismatch {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        call_args: Vec<ArgShape>,
        pin_clauses: Vec<Vec<ArgShape>>,
    },
    /// Patch references an MFA owned by the self-project but absent from the
    /// resolved self-snapshot. `suggested_source_for_prereq` is filled only
    /// with `--suggest-prereqs`.
    MissingPrereq {
        symbol: SymbolRef,
        self_branch: GitRef,
        suggested_source_for_prereq: Option<CommitSha>,
    },
    /// Pre-flight syntactic damage in a touched file: an unresolved
    /// merge-conflict marker, an exported function with no definition, etc.
    SyntacticArtifact {
        path: PathBuf,
        line: usize,
        artifact: ArtifactKind,
    },
    /// Patch changes a `-callback` signature; an implementer still has the old one.
    BehaviourCallbackSignatureChanged {
        behaviour: ModuleName,
        callback: FunctionName,
        arity: Arity,
        expected_after_patch: String,
        implementer: ModuleName,
        implementer_signature: String,
    },
    /// Patch removes a required callback; implementer still exports it.
    BehaviourCallbackRemoved {
        behaviour: ModuleName,
        callback: FunctionName,
        arity: Arity,
        implementer: ModuleName,
    },
    /// Patch adds a required callback; implementer is missing the export.
    BehaviourCallbackAdded {
        behaviour: ModuleName,
        callback: FunctionName,
        arity: Arity,
        implementer: ModuleName,
    },
    /// Patch touches a path absent on pin, but a module of the same basename exists elsewhere.
    ModuleRelocated {
        module: ModuleName,
        patch_path: PathBuf,
    },
    /// Wire-bearing macro value differs between source and pin.
    WireConstantChanged {
        module: ModuleName,
        macro_name: String,
        before: String,
        after: String,
    },
    /// `version/0` advanced without the matching historical-implementation module.
    HistoricalImplementationMissing {
        module: ModuleName,
        advertised_version_before: u32,
        advertised_version_after: u32,
        expected_historical_module: ModuleName,
    },
    /// Function bodies drifted at the same advertised `version/0`. Mixed-version cluster risk.
    WireContractBodyDrift {
        module: ModuleName,
        functions: Vec<FunctionName>,
        advertised_version: u32,
    },
    /// `version/0` decreased pin → patch.
    WireContractRegression {
        module: ModuleName,
        pin_version: u32,
        patch_version: u32,
    },
    /// Spec return shape differs between source and pin for a referenced MFA.
    ReturnShapeMismatch {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        source_signature: String,
        pin_signature: String,
    },
    /// A type-reference inside a `-spec`, `-callback`, `-type`, or
    /// `-opaque` attribute resolves to a name and arity that the pin
    /// does not expose via `-export_type`. Non-blocking: a missing
    /// type alias is a dialyzer-level concern, not a load-time crash.
    MissingType {
        module: ModuleName,
        name: TypeName,
        arity: Arity,
    },
    /// A touched path is absent from the target tree, but a
    /// configured `[[path_translation]]` rewrites it to a path that
    /// does exist on the target. Non-blocking: the operator (or the
    /// cherry-pick driver) rewrites the path before applying.
    PathRename {
        source_path: PathBuf,
        target_path: PathBuf,
        translation: TranslationSource,
    },
    /// A touched path is absent on the target while others are present
    /// and no `[[path_translation]]` rewrites it. Non-blocking: dropping
    /// the hunks for this path lets the rest apply. The all-absent case
    /// is `InapplicableReason::PathsMissingOnTarget`.
    TargetPathAbsent {
        path: RelativePath,
    },
    /// A `_SUITE.erl` file in the patch references `missing_module:f/n`
    /// where `missing_module` resolves to no source file under any
    /// configured `FamilyDefaults.test_helper_search_paths` on the
    /// target tree. The cherry-pick compiles and the SUITE may even
    /// load, but every call to the missing helper crashes at runtime:
    /// the failure pattern that bit the Jun 2026 v4.1.x → v4.0.x round
    /// (the `amqp_utils:connection_config/1` undef in
    /// `amqp10_connection_max_SUITE`). Non-blocking by default: the
    /// diagnostic equivalent (`Diagnostics.missing_test_modules`)
    /// always fires; the `Reason` variant fires only when
    /// `--target-repo-dir-path` is supplied and the resolution is
    /// authoritative.
    TestModuleSymbolMissing {
        suite_path: RelativePath,
        missing_module: ModuleName,
        call_sites: Vec<TestCallSite>,
    },
    /// A touched `.erl` declares `-behaviour(behaviour)` (or
    /// `-behavior(...)`) where `behaviour` resolves to no source
    /// module on the target tree. `erlc` compiles with a warning,
    /// then hook dispatch crashes at runtime. Sister verdict to the
    /// existing `BehaviourCallback*` variants which model implementer
    /// drift; this one catches the missing-behaviour-module case the
    /// existing variants do not.
    BehaviourModuleMissing {
        source_path: RelativePath,
        behaviour: ModuleName,
    },
    /// A touched `.erl` or `.hrl` declares `-include(path)` or
    /// `-include_lib(app/include/file.hrl)` against a target tree that
    /// does not ship the resolved header. Compile-time failure rather
    /// than runtime, but same user-facing shape as
    /// `TestModuleSymbolMissing`: looked fine on source, broken on
    /// target.
    HeaderFileMissing {
        source_path: RelativePath,
        include_directive: IncludeDirective,
        attempted_paths: Vec<RelativePath>,
    },
    /// A touched `.erl` or `.hrl` file references `?MACRO` on an added
    /// line, but `MACRO` is defined nowhere the target tree reaches (not
    /// in the file, its includes, or the patch itself) and is not
    /// predefined. Compiles on source, dangles on target. Non-blocking:
    /// the operator drops or adapts the reference.
    MacroUndefinedOnTarget {
        source_path: RelativePath,
        macro_name: String,
        line: u32,
    },
    /// A touched `.erl` or `.hrl` file uses `#record{}` on an added
    /// line, but `record` is defined nowhere the target tree reaches and
    /// is not added by the patch. Sister of `MacroUndefinedOnTarget` for
    /// the record namespace; non-blocking.
    RecordUndefinedOnTarget {
        source_path: RelativePath,
        record_name: RecordName,
        line: u32,
    },
    /// A touched `.erl` file calls an unqualified `f/a` on an added line
    /// that the target version of the same module neither defines,
    /// exports, imports, nor inherits as an auto-imported BIF. The
    /// function-namespace counterpart; non-blocking.
    LocalCallUndefinedOnTarget {
        source_path: RelativePath,
        function: FunctionName,
        arity: Arity,
        line: u32,
    },
    /// A touched `.erl` file calls a qualified `m:f/a` on an added line
    /// whose module is a first-party module present on the target tree,
    /// but the target version of that module does not export `f/a` (and
    /// the patch does not add it). The qualified counterpart of
    /// `LocalCallUndefinedOnTarget`, resolved against the called
    /// module's exports. Withheld when any pin snapshot already covers
    /// the module, when it is absent from the tree, or when its export
    /// surface is not fully readable. Non-blocking.
    QualifiedCallUndefinedOnTarget {
        source_path: RelativePath,
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        line: u32,
    },
    /// A qualified `m:f/a` call that resolves against the target tree
    /// has a target-tree `-spec` return shape that disagrees with its
    /// source-tree `-spec`. Sister of `ReturnShapeMismatch` for the
    /// live, snapshot-free first-party axis: same comparator,
    /// non-blocking like the rest of that axis. Only evaluated once the
    /// call is confirmed to resolve; withheld when either side lacks a
    /// `-spec`.
    QualifiedCallReturnShapeDrift {
        source_path: RelativePath,
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        source_signature: String,
        target_signature: String,
        line: u32,
    },
    /// Declared versioned-machine module is touched but the snapshot
    /// has no recorded `versioned_machine_version` on `side`.
    /// Non-blocking.
    VersionedMachineSnapshotMissing {
        module: ModuleName,
        side: SnapshotSide,
    },
    /// Declared wire constants are touched but the snapshot's
    /// `wire_constants` vector lacks the listed macros on `side`.
    /// `macros` is sorted alphabetically. Non-blocking.
    WireConstantBindingsMissing {
        module: ModuleName,
        macros: Vec<MacroName>,
        side: SnapshotSide,
    },
}

/// Which side of a snapshot comparison the data is missing from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSide {
    Source,
    Target,
    Both,
}

/// One `Module:Function/Arity` reference inside a `_SUITE.erl` that
/// resolves to a module absent on the target tree. Carries the line
/// number so the operator can jump straight to the call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TestCallSite {
    pub function: FunctionName,
    pub arity: Arity,
    pub line: u32,
}

/// `-include` and `-include_lib` are syntactically distinct: the
/// former takes a relative-to-source path, the latter an
/// `application/include/file.hrl` shape resolved through OTP's lib
/// path. The variant preserves which form the source used so the
/// remediation text can match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum IncludeDirective {
    Include { path: String },
    IncludeLib { path: String },
}

impl IncludeDirective {
    pub fn path(&self) -> &str {
        match self {
            Self::Include { path } | Self::IncludeLib { path } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranslationSource {
    ConfigStanza { name: String },
    ExternalFile { path: PathBuf, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "artifact_kind", rename_all = "snake_case")]
pub enum ArtifactKind {
    /// `<<<<<<<`, `=======`, `>>>>>>>`, or `||||||| ` marker present in a
    /// touched file. The cherry-pick driver left a conflict unresolved.
    ConflictMarker { marker: ConflictMarker },
    /// `-export([f/N])` line without a matching clause defining `f/N`.
    /// Compiles, then crashes at runtime on the first call.
    ExportWithoutBody {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ConflictMarker {
    /// `<<<<<<<` line: start of the local side.
    Ours,
    /// `=======` line: divider between sides.
    Divider,
    /// `>>>>>>>` line: end of the incoming side.
    Theirs,
    /// `||||||| ` line: base ancestor marker from a 3-way diff.
    Ancestor,
}

impl ConflictMarker {
    /// Returns the marker variant when `line` starts with at least seven
    /// of the same `git` conflict-marker token followed by whitespace or
    /// end of line. Returns `None` otherwise.
    ///
    /// `git`'s default marker length is seven; `merge.conflictStyle.markerSize`
    /// can grow it, so the check accepts seven or more.
    pub fn detect(line: &str) -> Option<Self> {
        Self::detect_with_marker(line, '<', Self::Ours)
            .or_else(|| Self::detect_with_marker(line, '=', Self::Divider))
            .or_else(|| Self::detect_with_marker(line, '>', Self::Theirs))
            .or_else(|| Self::detect_with_marker(line, '|', Self::Ancestor))
    }

    fn detect_with_marker(line: &str, token: char, marker: Self) -> Option<Self> {
        const MIN_RUN: usize = 7;
        let mut count = 0usize;
        for c in line.chars() {
            if c == token {
                count += 1;
                continue;
            }
            return if count >= MIN_RUN && (c == ' ' || c == '\t') {
                Some(marker)
            } else {
                None
            };
        }
        if count >= MIN_RUN { Some(marker) } else { None }
    }
}

/// The apply outcomes a divergence predictor consumes, ordered by
/// severity: a higher variant outranks a lower one on the same path, so
/// dedup to one conflict per path is a `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ApplyConflictKind {
    PostimageCollision,
    PreimageMissing,
    FileAbsent,
}

impl Reason {
    /// The risky apply outcome and its path, or `None` for a clean or
    /// non-apply reason. `PreimageDrifted` is excluded: a drifted preimage
    /// applies cleanly. Wildcard-free so a future apply reason must
    /// declare its risk here, not default silently to clean.
    pub fn apply_conflict(&self) -> Option<(&Path, ApplyConflictKind)> {
        match self {
            Self::PostimageCollision { path, .. } => {
                Some((path, ApplyConflictKind::PostimageCollision))
            }
            Self::PreimageMissing { path, .. } => Some((path, ApplyConflictKind::PreimageMissing)),
            Self::FileAbsent { path } => Some((path, ApplyConflictKind::FileAbsent)),
            Self::MissingSymbol { .. }
            | Self::ArityChanged { .. }
            | Self::SignatureChanged { .. }
            | Self::ContextDrift { .. }
            | Self::PreimageDrifted { .. }
            | Self::DeprecatedUsage { .. }
            | Self::NowHidden { .. }
            | Self::RecordFieldsChanged { .. }
            | Self::UnsupportedFileType { .. }
            | Self::UntrackedModuleMissing { .. }
            | Self::ClauseMismatch { .. }
            | Self::MissingPrereq { .. }
            | Self::SyntacticArtifact { .. }
            | Self::BehaviourCallbackSignatureChanged { .. }
            | Self::BehaviourCallbackRemoved { .. }
            | Self::BehaviourCallbackAdded { .. }
            | Self::ModuleRelocated { .. }
            | Self::WireConstantChanged { .. }
            | Self::HistoricalImplementationMissing { .. }
            | Self::WireContractBodyDrift { .. }
            | Self::WireContractRegression { .. }
            | Self::ReturnShapeMismatch { .. }
            | Self::MissingType { .. }
            | Self::PathRename { .. }
            | Self::TargetPathAbsent { .. }
            | Self::TestModuleSymbolMissing { .. }
            | Self::BehaviourModuleMissing { .. }
            | Self::HeaderFileMissing { .. }
            | Self::MacroUndefinedOnTarget { .. }
            | Self::RecordUndefinedOnTarget { .. }
            | Self::LocalCallUndefinedOnTarget { .. }
            | Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. }
            | Self::VersionedMachineSnapshotMissing { .. }
            | Self::WireConstantBindingsMissing { .. } => None,
        }
    }

    /// The class this reason flagged, for a predicted-class breakdown:
    /// the absent-symbol family only. Not the corpus break class, which
    /// is the symbol that actually broke the build (the observed outcome).
    pub fn resolver_class(&self) -> Option<ResolverClass> {
        match self {
            Self::MissingSymbol { symbol, .. } | Self::MissingPrereq { symbol, .. } => {
                Some(ResolverClass::of_symbol_kind(&symbol.kind))
            }
            Self::MissingType { .. } => Some(ResolverClass::Type),
            Self::MacroUndefinedOnTarget { .. } => Some(ResolverClass::Macro),
            Self::RecordUndefinedOnTarget { .. } => Some(ResolverClass::Record),
            Self::LocalCallUndefinedOnTarget { .. } => Some(ResolverClass::LocalCall),
            Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. } => Some(ResolverClass::QualifiedCall),
            Self::HeaderFileMissing { .. } => Some(ResolverClass::Include),
            Self::BehaviourModuleMissing { .. } => Some(ResolverClass::Behaviour),
            Self::ArityChanged { .. }
            | Self::SignatureChanged { .. }
            | Self::FileAbsent { .. }
            | Self::ContextDrift { .. }
            | Self::PreimageDrifted { .. }
            | Self::PostimageCollision { .. }
            | Self::PreimageMissing { .. }
            | Self::DeprecatedUsage { .. }
            | Self::NowHidden { .. }
            | Self::RecordFieldsChanged { .. }
            | Self::UnsupportedFileType { .. }
            | Self::UntrackedModuleMissing { .. }
            | Self::ClauseMismatch { .. }
            | Self::SyntacticArtifact { .. }
            | Self::BehaviourCallbackSignatureChanged { .. }
            | Self::BehaviourCallbackRemoved { .. }
            | Self::BehaviourCallbackAdded { .. }
            | Self::ModuleRelocated { .. }
            | Self::WireConstantChanged { .. }
            | Self::HistoricalImplementationMissing { .. }
            | Self::WireContractBodyDrift { .. }
            | Self::WireContractRegression { .. }
            | Self::ReturnShapeMismatch { .. }
            | Self::PathRename { .. }
            | Self::TargetPathAbsent { .. }
            | Self::TestModuleSymbolMissing { .. }
            | Self::VersionedMachineSnapshotMissing { .. }
            | Self::WireConstantBindingsMissing { .. } => None,
        }
    }

    // wildcard-free so a new variant fails to compile until classified
    pub fn is_blocking(&self) -> bool {
        match self {
            // A symbol that exists at a later tag is adaptable: land
            // the dep pin bump first.
            Self::MissingSymbol {
                first_seen_at_tag: Some(_),
                ..
            } => false,
            Self::ArityChanged {
                expected_available_at: Some(_),
                ..
            } => false,
            Self::MissingSymbol { .. }
            | Self::ArityChanged { .. }
            | Self::SignatureChanged { .. }
            | Self::FileAbsent { .. }
            | Self::NowHidden { .. }
            | Self::RecordFieldsChanged { .. }
            | Self::UntrackedModuleMissing { .. }
            | Self::ClauseMismatch { .. }
            | Self::MissingPrereq { .. }
            | Self::SyntacticArtifact { .. }
            | Self::BehaviourCallbackSignatureChanged { .. }
            | Self::BehaviourCallbackRemoved { .. }
            | Self::BehaviourCallbackAdded { .. }
            | Self::WireConstantChanged { .. }
            | Self::HistoricalImplementationMissing { .. }
            | Self::WireContractRegression { .. }
            | Self::ReturnShapeMismatch { .. } => true,
            Self::ContextDrift { .. }
            | Self::PreimageDrifted { .. }
            | Self::PreimageMissing { .. }
            | Self::PostimageCollision { .. }
            | Self::DeprecatedUsage { .. }
            | Self::UnsupportedFileType { .. }
            | Self::ModuleRelocated { .. }
            | Self::WireContractBodyDrift { .. }
            | Self::MissingType { .. }
            | Self::PathRename { .. }
            | Self::TargetPathAbsent { .. }
            | Self::TestModuleSymbolMissing { .. }
            | Self::BehaviourModuleMissing { .. }
            | Self::HeaderFileMissing { .. }
            | Self::MacroUndefinedOnTarget { .. }
            | Self::RecordUndefinedOnTarget { .. }
            | Self::LocalCallUndefinedOnTarget { .. }
            | Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. }
            | Self::VersionedMachineSnapshotMissing { .. }
            | Self::WireConstantBindingsMissing { .. } => false,
        }
    }

    /// True for reasons that are facts about *files* rather than the
    /// pin's API surface. These are the only reasons path routing may
    /// drop for a pin that owns none of the touched paths.
    /// Wildcard-free on purpose, like `is_blocking`.
    pub fn is_path_scoped(&self) -> bool {
        match self {
            Self::FileAbsent { .. }
            | Self::ModuleRelocated { .. }
            | Self::PreimageDrifted { .. }
            | Self::PreimageMissing { .. }
            | Self::PostimageCollision { .. }
            | Self::ContextDrift { .. }
            | Self::PathRename { .. }
            | Self::TargetPathAbsent { .. }
            | Self::UnsupportedFileType { .. }
            | Self::SyntacticArtifact { .. } => true,
            Self::MissingSymbol { .. }
            | Self::ArityChanged { .. }
            | Self::SignatureChanged { .. }
            | Self::DeprecatedUsage { .. }
            | Self::NowHidden { .. }
            | Self::RecordFieldsChanged { .. }
            | Self::UntrackedModuleMissing { .. }
            | Self::ClauseMismatch { .. }
            | Self::MissingPrereq { .. }
            | Self::BehaviourCallbackSignatureChanged { .. }
            | Self::BehaviourCallbackRemoved { .. }
            | Self::BehaviourCallbackAdded { .. }
            | Self::WireConstantChanged { .. }
            | Self::HistoricalImplementationMissing { .. }
            | Self::WireContractBodyDrift { .. }
            | Self::WireContractRegression { .. }
            | Self::ReturnShapeMismatch { .. }
            | Self::MissingType { .. }
            | Self::TestModuleSymbolMissing { .. }
            | Self::BehaviourModuleMissing { .. }
            | Self::HeaderFileMissing { .. }
            | Self::MacroUndefinedOnTarget { .. }
            | Self::RecordUndefinedOnTarget { .. }
            | Self::LocalCallUndefinedOnTarget { .. }
            | Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. }
            | Self::VersionedMachineSnapshotMissing { .. }
            | Self::WireConstantBindingsMissing { .. } => false,
        }
    }
}

/// The minimum pin that satisfies a missing-symbol reason: the typed
/// ordering payload q-port turns into a "bump must land first" edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct NeedsPin {
    pub project: ProjectName,
    pub tag: TagName,
}

/// Per-pin tally of file kinds the patch touched. Lets `promote_inapplicable`
/// tell a real green check from a diff with nothing analyzable to check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TouchedKinds {
    #[serde(default)]
    pub erl: u32,
    #[serde(default)]
    pub hrl: u32,
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub docs: u32,
    #[serde(default)]
    pub tests: u32,
    #[serde(default)]
    pub makefile: u32,
    #[serde(default)]
    pub mix_exs: u32,
    #[serde(default)]
    pub ci_workflow: u32,
    #[serde(default)]
    pub app_src: u32,
    #[serde(default)]
    pub rebar_config: u32,
    #[serde(default)]
    pub other: u32,
    /// Every changed `.erl` hunk is a Variant A unwrap of test-only
    /// `-ifdef(TEST)` machinery around an `-export` directive (see
    /// `InapplicableReason::OnlyTestVisibilityChanged`). When true,
    /// the patch touches Erlang surface but adds no semantic
    /// content, so an empty reason set should promote to
    /// `Inapplicable` instead of `Compatible`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_test_visibility: bool,
    /// The patch touches Erlang surface but references no tracked-dep
    /// API (every non-self pin sees zero in-scope references). The CLI
    /// sets this after `evaluate_series` when a self-pin is present
    /// and every non-self pin's `tracked_refs` is empty; see
    /// `InapplicableReason::OnlySelfSurfaceTouched`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_self_surface: bool,
}

// `&bool` is required by serde's `skip_serializing_if` predicate shape.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

impl TouchedKinds {
    pub fn is_empty(&self) -> bool {
        // Destructure so adding a new field forces this method to be updated.
        let Self {
            erl,
            hrl,
            schema,
            docs,
            tests,
            makefile,
            mix_exs,
            ci_workflow,
            app_src,
            rebar_config,
            other,
            only_test_visibility,
            only_self_surface,
        } = self;
        *erl == 0
            && *hrl == 0
            && *schema == 0
            && *docs == 0
            && *tests == 0
            && *makefile == 0
            && *mix_exs == 0
            && *ci_workflow == 0
            && *app_src == 0
            && *rebar_config == 0
            && *other == 0
            && !*only_test_visibility
            && !*only_self_surface
    }

    pub fn classify(path: &Path) -> FileKind {
        let p = path.to_string_lossy();
        let lower = p.to_ascii_lowercase();
        let is_erl_source = lower.ends_with(".erl") || lower.ends_with(".hrl");
        // a docs/ path component must not reclassify real Erlang source as
        // docs: that would vacuously skip an API-affecting change
        if lower.ends_with(".md")
            || lower.ends_with(".adoc")
            || lower.ends_with(".rst")
            || lower.ends_with(".txt")
            || (!is_erl_source && (lower.contains("/docs/") || lower.starts_with("docs/")))
        {
            return FileKind::Docs;
        }
        if lower.contains("_suite_data/")
            || lower.contains("/test/")
            || lower.contains("/tests/")
            || lower.starts_with("test/")
            || lower.starts_with("tests/")
            || lower.ends_with("_suite.erl")
        {
            return FileKind::Tests;
        }
        if lower.ends_with(".schema") || lower.ends_with(".snippets") {
            return FileKind::Schema;
        }
        if lower.ends_with(".erl") {
            return FileKind::Erl;
        }
        if lower.ends_with(".hrl") {
            return FileKind::Hrl;
        }
        if lower.ends_with(".app.src") || lower.ends_with(".app.src.script") {
            return FileKind::AppSrc;
        }
        if lower.ends_with("rebar.config") || lower.ends_with("rebar3.config") {
            return FileKind::RebarConfig;
        }
        if lower.ends_with("/mix.exs")
            || lower == "mix.exs"
            || lower.ends_with("/mix.lock")
            || lower == "mix.lock"
        {
            return FileKind::MixExs;
        }
        if Self::is_ci_workflow(&lower) {
            return FileKind::CiWorkflow;
        }
        if Self::is_makefile(&lower) {
            return FileKind::Makefile;
        }
        FileKind::Other
    }

    fn is_ci_workflow(lower: &str) -> bool {
        if lower.contains(".github/workflows/") {
            return lower.ends_with(".yml") || lower.ends_with(".yaml");
        }
        lower.contains(".github/actions/")
    }

    fn is_makefile(lower: &str) -> bool {
        if lower.ends_with(".mk") {
            return true;
        }
        let basename = lower.rsplit('/').next().unwrap_or(lower);
        matches!(
            basename,
            "makefile" | "erlang.mk" | "rabbitmq-components.mk"
        )
    }

    pub fn record(&mut self, kind: FileKind) {
        match kind {
            FileKind::Erl => self.erl += 1,
            FileKind::Hrl => self.hrl += 1,
            FileKind::Schema => self.schema += 1,
            FileKind::Docs => self.docs += 1,
            FileKind::Tests => self.tests += 1,
            FileKind::Makefile => self.makefile += 1,
            FileKind::MixExs => self.mix_exs += 1,
            FileKind::CiWorkflow => self.ci_workflow += 1,
            FileKind::AppSrc => self.app_src += 1,
            FileKind::RebarConfig => self.rebar_config += 1,
            FileKind::Other => self.other += 1,
        }
    }

    pub fn from_paths<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut tk = Self::default();
        for p in paths {
            tk.record(Self::classify(p.as_ref()));
        }
        tk
    }

    /// `None` when any `.erl` or `.hrl` was touched: an analyzable diff with
    /// zero reasons is real `Compatible`, not `Inapplicable`.
    pub fn inapplicable_reason(&self) -> Option<InapplicableReason> {
        if self.only_test_visibility {
            return Some(InapplicableReason::OnlyTestVisibilityChanged);
        }
        if self.only_self_surface {
            return Some(InapplicableReason::OnlySelfSurfaceTouched);
        }
        if self.erl > 0 || self.hrl > 0 {
            return None;
        }
        if self.is_empty() {
            return Some(InapplicableReason::NoErlangSurfaceTouched);
        }
        if self.is_only(|tk| tk.docs > 0) {
            return Some(InapplicableReason::OnlyDocsTouched);
        }
        if self.is_only(|tk| tk.tests > 0) {
            return Some(InapplicableReason::OnlyTestFixturesTouched);
        }
        if self.is_only(|tk| tk.schema > 0) {
            return Some(InapplicableReason::OnlySchemaTouched);
        }
        if self.is_only(|tk| tk.makefile > 0) {
            return Some(InapplicableReason::OnlyMakefileTouched);
        }
        if self.is_only(|tk| tk.mix_exs > 0) {
            return Some(InapplicableReason::OnlyMixExsTouched);
        }
        if self.is_only(|tk| tk.ci_workflow > 0) {
            return Some(InapplicableReason::OnlyCiWorkflowTouched);
        }
        if self.is_only(|tk| tk.app_src > 0) {
            return Some(InapplicableReason::OnlyAppSrcTouched);
        }
        if self.is_only(|tk| tk.rebar_config > 0) {
            return Some(InapplicableReason::OnlyRebarConfigTouched);
        }
        Some(InapplicableReason::NoErlangSurfaceTouched)
    }

    /// True when exactly the bucket selected by `selector` is non-zero
    /// and every other (non-Erlang-surface) bucket is zero.
    fn is_only<F: Fn(&Self) -> bool>(&self, selector: F) -> bool {
        if !selector(self) {
            return false;
        }
        let buckets = [
            self.schema > 0,
            self.docs > 0,
            self.tests > 0,
            self.makefile > 0,
            self.mix_exs > 0,
            self.ci_workflow > 0,
            self.app_src > 0,
            self.rebar_config > 0,
            self.other > 0,
        ];
        let active = buckets.iter().filter(|b| **b).count();
        active == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Erl,
    Hrl,
    Schema,
    Docs,
    Tests,
    Makefile,
    MixExs,
    CiWorkflow,
    AppSrc,
    RebarConfig,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PinVerdict {
    pub pin: Pin,
    pub verdict: Verdict,
    #[serde(default)]
    pub tracked_refs: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_ref_details: Vec<SymbolRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_delta_details: Vec<SourceDelta>,
    #[serde(default, skip_serializing_if = "TouchedKinds::is_empty")]
    pub touched: TouchedKinds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SourceDelta {
    pub module: ModuleName,
    pub function: FunctionName,
    pub arity: Arity,
    pub source_spec: String,
    pub target_spec: String,
}

impl PinVerdict {
    pub fn new(pin: Pin, verdict: Verdict) -> Self {
        Self {
            pin,
            verdict,
            tracked_refs: 0,
            tracked_ref_details: Vec::new(),
            source_delta_details: Vec::new(),
            touched: TouchedKinds::default(),
        }
    }

    #[must_use]
    pub fn with_tracked_refs(mut self, n: usize) -> Self {
        self.tracked_refs = n;
        self
    }

    #[must_use]
    pub fn with_tracked_ref_details(mut self, details: Vec<SymbolRef>) -> Self {
        self.tracked_refs = details.len();
        self.tracked_ref_details = details;
        self
    }

    #[must_use]
    pub fn with_source_delta_details(mut self, deltas: Vec<SourceDelta>) -> Self {
        self.source_delta_details = deltas;
        self
    }

    #[must_use]
    pub fn with_touched(mut self, touched: TouchedKinds) -> Self {
        self.touched = touched;
        self
    }
}

/// Saturating sum of `tracked_refs` over pins not owned by a self project.
#[must_use]
pub fn non_self_tracked(results: &[PinVerdict], self_projects: &BTreeSet<ProjectName>) -> u32 {
    results
        .iter()
        .filter(|pin| !self_projects.contains(&pin.pin.project))
        .fold(0u32, |acc, pin| {
            acc.saturating_add(u32::try_from(pin.tracked_refs).unwrap_or(u32::MAX))
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SeriesVerdict {
    pub results: Vec<PinVerdict>,
    pub summary: SeriesSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SeriesSummary {
    pub compatible: u32,
    pub requires_adaptation: u32,
    pub incompatible: u32,
    #[serde(default)]
    pub inapplicable: u32,
}

impl SeriesSummary {
    /// True when any pin needs attention: the one rule both the series
    /// exit code and the round clearance gate on.
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        self.incompatible > 0 || self.requires_adaptation > 0
    }
}

// 0 every pin clean; 3 partial-success when any pin needs attention
pub mod exit {
    pub const OK: i32 = 0;
    pub const NEEDS_ATTENTION: i32 = 3;
}

impl SeriesVerdict {
    pub fn from_results(results: Vec<PinVerdict>) -> Self {
        let summary = summary_of(&results);
        Self { results, summary }
    }

    /// Re-derives the summary after per-pin verdicts were mutated in
    /// place (path routing, availability reclassification).
    pub fn recompute_summary(&mut self) {
        self.summary = summary_of(&self.results);
    }

    /// Promote each `Compatible`-with-zero-refs pin to `Inapplicable` when its
    /// `TouchedKinds` indicates no analyzable Erlang surface. The summary
    /// is recomputed from the rewritten results.
    pub fn promote_inapplicable(self) -> Self {
        let results: Vec<PinVerdict> = self
            .results
            .into_iter()
            .map(|pv| {
                if matches!(pv.verdict, Verdict::Compatible) && pv.tracked_refs == 0 {
                    if let Some(reason) = pv.touched.inapplicable_reason() {
                        return PinVerdict {
                            verdict: Verdict::Inapplicable { reason },
                            ..pv
                        };
                    }
                }
                pv
            })
            .collect();
        Self::from_results(results)
    }

    // 0 when every pin is Compatible or Inapplicable; 3 otherwise
    pub fn worst_exit_code(&self) -> i32 {
        if self.summary.is_blocking() {
            exit::NEEDS_ATTENTION
        } else {
            exit::OK
        }
    }
}

/// Series-wide diagnostic envelope. Strictly separate from `Verdict`
/// so untracked-call signals never leak into machine-readable verdicts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Diagnostics {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub untracked_calls: BTreeMap<ModuleName, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub untracked_records: BTreeMap<RecordName, usize>,
    /// In-scope references on *context* lines that do not resolve at
    /// some pin, per module. Pre-existing target facts: never reasons.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context_refs_missing: BTreeMap<ModuleName, usize>,
    /// Touched paths no configured project owns, tallied by their
    /// first two path components. The breadcrumb behind an
    /// `Inapplicable { Untracked }` verdict.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unattributed_paths: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Unanalyzed::is_empty")]
    pub unanalyzed: Unanalyzed,
    /// SUITE names worth running for the patch, derived from touched paths.
    /// Populated even when the verdict promotes to `Inapplicable` so that
    /// schema-only and config-only diffs still surface relevant suites.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_suites: Vec<String>,
    /// Per-`_SUITE.erl` map of `helper_module -> call_site_count`. The
    /// always-on diagnostic counterpart of
    /// `Reason::TestModuleSymbolMissing`: emitted by the parse pass
    /// even without a target tree (which is why the inner key is the
    /// `ModuleName` rather than a resolved-or-not bit). When
    /// `--target-repo-dir-path` is supplied the resolver re-walks this
    /// map and promotes the absent ones to typed reasons. The nested
    /// `BTreeMap` shape keeps the JSON envelope serialisable without
    /// a custom composite-key encoder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub missing_test_modules: BTreeMap<RelativePath, BTreeMap<ModuleName, usize>>,
    /// Advisory: the patch's content appears to already exist on the
    /// target (tier-1 identical patch, tier-2 content tally, or both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub already_present: Option<AlreadyPresent>,
    /// Why already-present detection was skipped, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub already_present_skipped: Option<AlreadyPresentSkipped>,
    /// Dep pins that diverge between source and target trees and are
    /// reachable from the apps this patch touches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dep_pin_divergence: Vec<DepPinDivergence>,
    /// Dep pins this patch itself changes or introduces in the root
    /// `rabbitmq-components.mk`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pin_bumps: Vec<PinBump>,
    /// What the live qualified-call gate's return-shape check did with
    /// each first-party resolved call: compared, or withheld and why.
    #[serde(default, skip_serializing_if = "ShapeCheckTally::is_empty")]
    pub qualified_call_shape_checks: ShapeCheckTally,
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        // Destructure so adding a field forces this method to be updated.
        let Self {
            untracked_calls,
            untracked_records,
            context_refs_missing,
            unattributed_paths,
            unanalyzed,
            suggested_suites,
            missing_test_modules,
            already_present,
            already_present_skipped,
            dep_pin_divergence,
            pin_bumps,
            qualified_call_shape_checks,
        } = self;
        untracked_calls.is_empty()
            && untracked_records.is_empty()
            && context_refs_missing.is_empty()
            && unattributed_paths.is_empty()
            && unanalyzed.is_empty()
            && suggested_suites.is_empty()
            && missing_test_modules.is_empty()
            && already_present.is_none()
            && already_present_skipped.is_none()
            && dep_pin_divergence.is_empty()
            && pin_bumps.is_empty()
            && qualified_call_shape_checks.is_empty()
    }

    /// Record that `suite` references `helper`; bumps the call-site
    /// counter. Convenience for the parse pass.
    pub fn record_missing_test_module(&mut self, suite: RelativePath, helper: ModuleName) {
        let inner = self.missing_test_modules.entry(suite).or_default();
        *inner.entry(helper).or_insert(0) += 1;
    }
}

/// How a tier-1 already-present match against the target branch was
/// established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TargetMatchKind {
    /// A walked target commit's `-x` trailer names the candidate (or
    /// one of its trailer origins or inner PR commits).
    TrailerIntersection,
    /// One of the candidate's own trailer origins is already an
    /// ancestor of the target tip.
    TrailerAncestry,
    /// A walked target commit's normalized patch hash equals the
    /// candidate's.
    PatchId,
}

/// A target commit found to carry the candidate's patch. The subject
/// rides along so consumers can auto-fill exclusion reasons without
/// a second git query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TargetMatch {
    pub commit: CommitSha,
    pub via: TargetMatchKind,
    pub subject: String,
}

/// What the live qualified-call gate's return-shape check did with
/// each first-party resolved call. Never reasons: a withhold is not a
/// finding, but a round where most calls are shape-blind must read
/// differently from a round where all of them were compared.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ShapeCheckTally {
    /// Both sides had a `-spec` that parsed to a known type; the shapes
    /// were compared. Drift is visible in the reasons, not double-counted
    /// here.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub compared: usize,
    /// One or both sides declare no `-spec` for the called function.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_no_spec: usize,
    /// Both sides have a spec but at least one parses to
    /// `SpecType::Unknown`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_unknown_type: usize,
    /// The callee module was not readable on the source side: no
    /// checkout was supplied, or the checkout has no file at the
    /// module's path (a defaulted `--repo-dir-path` that is not the
    /// source tree lands here, not in `withheld_no_spec`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_no_source: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl ShapeCheckTally {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Per-file hunk counters for the content-presence check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HunkTally {
    pub applied: usize,
    pub considered: usize,
    pub ambiguous: usize,
    pub low_confidence: usize,
}

/// Tier-2 advisory: how much of the patch's post-image already exists
/// in the named pin's tree. A tally, never a boolean: partial presence
/// is common and the fraction is the signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ContentPresence {
    /// The pin whose evaluation produced the tally. In cross-branch
    /// mode the self pin resolves to the target branch, so its tally
    /// is the target-tree answer.
    pub pin: ProjectName,
    pub hunks_already_applied: usize,
    pub hunks_considered: usize,
    pub hunks_ambiguous: usize,
    pub hunks_low_confidence: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_file: BTreeMap<RelativePath, HunkTally>,
}

impl ContentPresence {
    /// Every considered hunk is already applied: the strong signal.
    pub fn fully_present(&self) -> bool {
        self.hunks_considered > 0 && self.hunks_already_applied == self.hunks_considered
    }
}

/// Advisory: the patch's content appears to already exist on the
/// target. Never affects the verdict; q-port and the operator own
/// the exclusion decision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AlreadyPresent {
    /// Tier-1 hit: an identical patch landed on the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identical: Option<TargetMatch>,
    /// Tier-2 tally: the post-image is (partly) present in the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentPresence>,
}

impl AlreadyPresent {
    pub fn is_empty(&self) -> bool {
        self.identical.is_none() && self.content.is_none()
    }
}

/// A dependency whose `rabbitmq-components.mk` pin differs between
/// the source commit's tree and the target tree, and that an app
/// touched by the patch (transitively) depends on. Advisory only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DepPinDivergence {
    pub dep: DependencyName,
    /// Pin at the source commit, e.g. `hex 2.16.0`.
    pub source: String,
    /// Pin at the target tree, e.g. `hex 2.13.0`.
    pub target: String,
}

/// A dep pin the candidate commit itself changes or introduces in the
/// root `rabbitmq-components.mk`. Advisory only: the verdict on a
/// bump-only diff stays `Inapplicable`, this is the evidence behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PinBump {
    pub dep: DependencyName,
    /// Pin before the commit, e.g. `hex 2.16.0`; `None` when the
    /// commit introduces the pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Pin after the commit, e.g. `hex 2.17.1`.
    pub to: String,
    /// Snapshot-store assessment of the bumped-to version. Detection
    /// is a pure function of the patch and may be cached; the store's
    /// contents are not a cache-key input, so this field is filled
    /// post-cache and cached values carry `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<BumpStatus>,
}

/// What the snapshot store knows about a bumped-to pin version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BumpStatus {
    /// The bumped dep is not a tracked project; nothing to vet.
    Untracked,
    /// No snapshot for the bumped-to version; `note` carries the remedy.
    SnapshotMissing {
        note: String,
    },
    SnapshotPresent,
}

/// Why already-present detection could not run. Emitted instead of
/// silence: an invisible skip would read as "not present".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum AlreadyPresentSkipped {
    /// The candidate commit object is absent from the target object
    /// store (an unfetched branch, usually).
    CandidateMissingFromTargetOdb { sha: CommitSha },
    /// The candidate and the target tip share no history.
    NoMergeBase,
    /// The target-side walk failed (shallow clone, corrupt object).
    TargetWalkFailed { detail: String },
}

/// Counts of call sites the analyzer could not resolve: `apply/3`-style
/// BIFs, and dispatch through a variable module or function. Informational only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Unanalyzed {
    #[serde(default)]
    pub apply: usize,
    #[serde(default)]
    pub variable_dispatch: usize,
}

impl Unanalyzed {
    pub fn is_empty(&self) -> bool {
        self.apply == 0 && self.variable_dispatch == 0
    }
}

fn summary_of(results: &[PinVerdict]) -> SeriesSummary {
    let mut summary = SeriesSummary::default();
    for r in results {
        match r.verdict {
            Verdict::Compatible => summary.compatible += 1,
            Verdict::RequiresAdaptation { .. } => summary.requires_adaptation += 1,
            Verdict::Incompatible { .. } => summary.incompatible += 1,
            Verdict::Inapplicable { .. } => summary.inapplicable += 1,
        }
    }
    summary
}

/// One symbol-availability question the post-cache probe answers:
/// "at which snapshot tag strictly later than this pin does the
/// symbol first appear?"
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AvailabilityQuery {
    ExactArity {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
    },
    AnyArity {
        module: ModuleName,
        function: FunctionName,
    },
    Record {
        name: RecordName,
    },
}

impl AvailabilityQuery {
    fn for_symbol(symbol: &SymbolRef) -> Option<Self> {
        match &symbol.kind {
            SymbolKind::Function { mfa } => Some(Self::ExactArity {
                module: mfa.module.clone(),
                function: mfa.function.clone(),
                arity: mfa.arity,
            }),
            SymbolKind::FunctionAnyArity { module, function } => Some(Self::AnyArity {
                module: module.clone(),
                function: function.clone(),
            }),
            SymbolKind::Record { name } => Some(Self::Record { name: name.clone() }),
            SymbolKind::Behaviour { .. }
            | SymbolKind::Callback { .. }
            | SymbolKind::Macro { .. }
            | SymbolKind::Type { .. } => None,
        }
    }
}

/// The output of `Patch::evaluate_series`: verdicts plus diagnostics plus
/// `PatchFacts` (source classifiers that drive downstream policy without
/// being verdict reasons).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SeriesEvaluation {
    pub verdict: SeriesVerdict,
    pub diagnostics: Diagnostics,
    #[serde(default, skip_serializing_if = "PatchFacts::is_empty")]
    pub patch_facts: PatchFacts,

    /// Paths the analysed patch touched, in diff-encounter order.
    /// Always emitted; an empty vec means the patch touched zero files.
    #[serde(default)]
    pub touched_paths: Vec<RelativePath>,

    /// Inner PR-branch commits for 2-parent merge SHAs. `None` for
    /// non-2-parent merges and non-merge SHAs.
    ///
    /// Do NOT add `skip_serializing_if = "Option::is_none"`. The
    /// `None` vs `Some(vec![])` distinction is wire-load-bearing.
    #[serde(default)]
    pub pr_commits: Option<Vec<PrCommit>>,
}

impl SeriesEvaluation {
    pub fn worst_exit_code(&self) -> i32 {
        self.verdict.worst_exit_code()
    }

    /// Populates symbol-availability fields on missing-symbol and
    /// arity-changed reasons, then reclassifies the affected pin
    /// verdicts (`is_blocking` is field-sensitive). `lookup` answers
    /// "earliest snapshot tag strictly later than this pin where the
    /// queried symbol exists", or `None` (including for self pins).
    /// Runs post-cache: the project's tag set is not a cache-key
    /// input, so cached values must stay unclassified.
    pub fn apply_symbol_availability<F>(&mut self, mut lookup: F)
    where
        F: FnMut(&Pin, &AvailabilityQuery) -> Option<TagName>,
    {
        let mut any_changed = false;
        for pv in &mut self.verdict.results {
            let pin = pv.pin.clone();
            let reasons = match &mut pv.verdict {
                Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
                    reasons
                }
                Verdict::Compatible | Verdict::Inapplicable { .. } => continue,
            };
            let mut changed = false;
            for reason in reasons.iter_mut() {
                match reason {
                    Reason::MissingSymbol {
                        symbol,
                        first_seen_at_tag: first_seen @ None,
                        needs_pin_at_least,
                        ..
                    } => {
                        let Some(query) = AvailabilityQuery::for_symbol(symbol) else {
                            continue;
                        };
                        if let Some(tag) = lookup(&pin, &query) {
                            *first_seen = Some(tag.clone());
                            *needs_pin_at_least = Some(NeedsPin {
                                project: pin.project.clone(),
                                tag,
                            });
                            changed = true;
                        }
                    }
                    Reason::ArityChanged {
                        module,
                        function,
                        expected,
                        expected_available_at: available @ None,
                        needs_pin_at_least,
                        ..
                    } => {
                        let query = AvailabilityQuery::ExactArity {
                            module: module.clone(),
                            function: function.clone(),
                            arity: *expected,
                        };
                        if let Some(tag) = lookup(&pin, &query) {
                            *available = Some(tag.clone());
                            *needs_pin_at_least = Some(NeedsPin {
                                project: pin.project.clone(),
                                tag,
                            });
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                let taken = mem::take(reasons);
                pv.verdict = Verdict::from_reasons(taken);
                any_changed = true;
            }
        }
        if any_changed {
            self.verdict.recompute_summary();
        }
    }
}

/// Source classifiers on the patch. Strictly separate from `Verdict` reasons.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PatchFacts {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub logging_style: BTreeMap<PathBuf, FileLoggingStyle>,
    #[serde(default, skip_serializing_if = "KhepriSignals::is_empty")]
    pub khepri_signals: KhepriSignals,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub introduces_versioned_record: BTreeSet<RecordName>,
}

impl PatchFacts {
    pub fn is_empty(&self) -> bool {
        self.logging_style.is_empty()
            && self.khepri_signals.is_empty()
            && self.introduces_versioned_record.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoggingStyle {
    LoggerMacros,
    RabbitLogModule,
    Mixed,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FileLoggingStyle {
    pub dominant: LoggingStyle,
    pub logger_macro_sites: usize,
    pub rabbit_log_sites: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KhepriSignals {
    #[serde(default)]
    pub touches_khepri_module: bool,
    #[serde(default)]
    pub uses_khepri_macros: bool,
    #[serde(default)]
    pub touches_only_khepri_branch: bool,
    #[serde(default)]
    pub touches_dual_branch: bool,
}

impl KhepriSignals {
    // `&self` is required by serde `skip_serializing_if`.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn is_empty(&self) -> bool {
        !self.touches_khepri_module
            && !self.uses_khepri_macros
            && !self.touches_only_khepri_branch
            && !self.touches_dual_branch
    }
}
