// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Verdict` and `Reason` with their classification methods.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::InapplicableReason;
use crate::compat::arg_shape::ArgShape;
use crate::model::names::{
    Arity, CommitSha, FieldName, FunctionName, GitRef, MacroName, ModuleName, ProjectName,
    RecordName, RelativePath, TagName, TypeName,
};
use crate::model::resolver_coverage::ResolverClass;
use crate::model::symbol::SymbolRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Compatible,
    RequiresAdaptation { reasons: Vec<Reason> },
    Incompatible { reasons: Vec<Reason> },
    Inapplicable { reason: InapplicableReason },
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
    /// lacked: an add/add a contiguous preimage match cannot detect. The
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
    /// are concrete enough to compare; `Unknown` on either side
    /// suppresses the reason.
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
    /// the failure pattern seen in the Jun 2026 v4.1.x → v4.0.x round
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
    /// then hook dispatch crashes at runtime. The `BehaviourCallback*`
    /// variants model implementer drift; this one catches the
    /// missing-behaviour-module case they do not.
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
    /// is not added by the patch. Same check as `MacroUndefinedOnTarget`
    /// for the record namespace; non-blocking.
    RecordUndefinedOnTarget {
        source_path: RelativePath,
        record_name: RecordName,
        line: u32,
    },
    /// A touched `.erl` file calls an unqualified `f/a` on an added line
    /// that the target version of the same module neither defines,
    /// exports, imports, nor inherits as an auto-imported BIF. The
    /// same check for the function namespace; non-blocking.
    LocalCallUndefinedOnTarget {
        source_path: RelativePath,
        function: FunctionName,
        arity: Arity,
        line: u32,
    },
    /// A touched `.erl` file calls a qualified `m:f/a` on an added line
    /// whose module is a first-party module present on the target tree,
    /// but the target version of that module does not export `f/a` (and
    /// the patch does not add it). The qualified form of
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
    /// source-tree `-spec`. Same comparison as `ReturnShapeMismatch` on
    /// the live, snapshot-free first-party axis: same comparator,
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
    /// An unqualified call that resolves against the target module's
    /// own definitions has a target-tree `-spec` return shape that
    /// disagrees with its source-tree `-spec`. The local-call form of
    /// `QualifiedCallReturnShapeDrift`; imported resolutions are
    /// withheld, since the callee's spec is in another module.
    LocalCallReturnShapeDrift {
        source_path: RelativePath,
        function: FunctionName,
        arity: Arity,
        source_signature: String,
        target_signature: String,
        line: u32,
    },
    /// A `?MACRO` the patch uses is defined in the touched file itself
    /// on both trees, exactly once each, and the normalized `-define`
    /// bodies disagree: the compile-clean, behavior-divergent class.
    /// Withheld for definitions reached any other way. Non-blocking.
    MacroValueDrift {
        source_path: RelativePath,
        macro_name: String,
        source_value: String,
        target_value: String,
        line: u32,
    },
    /// The target's version of a behaviour the patch declares requires
    /// a callback the source's version does not, and the target does
    /// not list it as optional: an implementation written against the
    /// source is missing it. `line` is the `-behaviour` attribute's
    /// line in the touched file. Non-blocking.
    BehaviourCallbackAddedOnTarget {
        source_path: RelativePath,
        behaviour: ModuleName,
        callback: FunctionName,
        arity: Arity,
        line: u32,
    },
    /// A callback both trees' versions of a declared behaviour declare
    /// has a different normalized signature on the target. The whole
    /// signature is compared, not just the return half: a callback
    /// contract's argument types are the implementer's obligation.
    /// Non-blocking.
    BehaviourCallbackDriftOnTarget {
        source_path: RelativePath,
        behaviour: ModuleName,
        callback: FunctionName,
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
    /// A first-party `m:f/a` referenced as arguments to a mocking or
    /// rpc form (a meck expectation, `rpc:call`, `erpc:call`, or a CT
    /// broker rpc helper) is neither exported nor defined by the target
    /// tree's module. Non-blocking: a meck expectation can install a
    /// function the module never had, so absence proves drift, not
    /// failure.
    IndirectCallUndefinedOnTarget {
        source_path: RelativePath,
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        via: IndirectCallForm,
        line: u32,
    },
    /// A touched `.erl` file adds an `-export_type([t/a])` entry naming
    /// a type neither the target version of the same module declares
    /// nor the patch adds. An `erlc` error, unlike `MissingType`, which
    /// is dialyzer-level; non-blocking regardless, being a target-tree
    /// finding rather than a pin verdict. Withheld when the target file
    /// is unreadable or declares a `parse_transform`, or when the added
    /// list names a macro.
    ExportedTypeUndefinedOnTarget {
        source_path: RelativePath,
        type_name: TypeName,
        arity: Arity,
        line: u32,
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

impl ApplyConflictKind {
    /// Stable label used in clearance roll-ups and text output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostimageCollision => "postimage_collision",
            Self::PreimageMissing => "preimage_missing",
            Self::FileAbsent => "file_absent",
        }
    }
}

/// Which recognized call form carried an indirect MFA reference. Names
/// the innermost form: a meck expectation reached through an rpc helper
/// is `MeckExpect`, not the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum IndirectCallForm {
    MeckExpect,
    RpcCall,
    ErpcCall,
    CtBrokerHelperRpc,
    RabbitMiscRpcCall,
}

impl IndirectCallForm {
    /// The Erlang form the extractor matched, for text output. Distinct
    /// from the serde wire name on purpose.
    #[must_use]
    pub fn display_form(self) -> &'static str {
        match self {
            Self::MeckExpect => "meck:expect",
            Self::RpcCall => "rpc:call",
            Self::ErpcCall => "erpc:call",
            Self::CtBrokerHelperRpc => "rabbit_ct_broker_helpers:rpc",
            Self::RabbitMiscRpcCall => "rabbit_misc:rpc_call",
        }
    }
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
            | Self::ExportedTypeUndefinedOnTarget { .. }
            | Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::IndirectCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. }
            | Self::LocalCallReturnShapeDrift { .. }
            | Self::MacroValueDrift { .. }
            | Self::BehaviourCallbackAddedOnTarget { .. }
            | Self::BehaviourCallbackDriftOnTarget { .. }
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
            Self::MacroUndefinedOnTarget { .. } | Self::MacroValueDrift { .. } => {
                Some(ResolverClass::Macro)
            }
            Self::RecordUndefinedOnTarget { .. } => Some(ResolverClass::Record),
            Self::ExportedTypeUndefinedOnTarget { .. } => Some(ResolverClass::Type),
            Self::LocalCallUndefinedOnTarget { .. } | Self::LocalCallReturnShapeDrift { .. } => {
                Some(ResolverClass::LocalCall)
            }
            Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. } => Some(ResolverClass::QualifiedCall),
            Self::IndirectCallUndefinedOnTarget { .. } => Some(ResolverClass::IndirectCall),
            Self::HeaderFileMissing { .. } => Some(ResolverClass::Include),
            Self::BehaviourModuleMissing { .. }
            | Self::BehaviourCallbackAddedOnTarget { .. }
            | Self::BehaviourCallbackDriftOnTarget { .. } => Some(ResolverClass::Behaviour),
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
            // exists at a later tag: adaptable once the dep pin bump lands
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
            | Self::ExportedTypeUndefinedOnTarget { .. }
            | Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::IndirectCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. }
            | Self::LocalCallReturnShapeDrift { .. }
            | Self::MacroValueDrift { .. }
            | Self::BehaviourCallbackAddedOnTarget { .. }
            | Self::BehaviourCallbackDriftOnTarget { .. }
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
            | Self::ExportedTypeUndefinedOnTarget { .. }
            | Self::QualifiedCallUndefinedOnTarget { .. }
            | Self::IndirectCallUndefinedOnTarget { .. }
            | Self::QualifiedCallReturnShapeDrift { .. }
            | Self::LocalCallReturnShapeDrift { .. }
            | Self::MacroValueDrift { .. }
            | Self::BehaviourCallbackAddedOnTarget { .. }
            | Self::BehaviourCallbackDriftOnTarget { .. }
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
