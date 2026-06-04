// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::compat::arg_shape::ArgShape;
use crate::model::names::{
    Arity, CommitSha, FieldName, FunctionName, GitRef, ModuleName, ProjectName, RecordName,
    RelativePath, TagName, TypeName,
};
use crate::model::pin::Pin;
use crate::model::pr_commit::PrCommit;
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
        first_seen_at_tag: Option<TagName>,
        suggested_replacement: Option<SymbolRef>,
    },
    ArityChanged {
        module: ModuleName,
        function: FunctionName,
        expected: Arity,
        found: Vec<Arity>,
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

impl Reason {
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
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
                | Self::ReturnShapeMismatch { .. }
        )
    }
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
        if lower.ends_with(".md")
            || lower.ends_with(".adoc")
            || lower.ends_with(".rst")
            || lower.ends_with(".txt")
            || lower.contains("/docs/")
            || lower.starts_with("docs/")
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

// 0 every pin clean; 3 partial-success when any pin needs attention
pub mod exit {
    pub const OK: i32 = 0;
    pub const NEEDS_ATTENTION: i32 = 3;
}

impl SeriesVerdict {
    pub fn from_results(results: Vec<PinVerdict>) -> Self {
        let mut summary = SeriesSummary::default();
        for r in &results {
            match r.verdict {
                Verdict::Compatible => summary.compatible += 1,
                Verdict::RequiresAdaptation { .. } => summary.requires_adaptation += 1,
                Verdict::Incompatible { .. } => summary.incompatible += 1,
                Verdict::Inapplicable { .. } => summary.inapplicable += 1,
            }
        }
        Self { results, summary }
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
        if self.summary.incompatible > 0 || self.summary.requires_adaptation > 0 {
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
    #[serde(default, skip_serializing_if = "Unanalyzed::is_empty")]
    pub unanalyzed: Unanalyzed,
    /// SUITE names worth running for the patch, derived from touched paths.
    /// Populated even when the verdict promotes to `Inapplicable` so that
    /// schema-only and config-only diffs still surface relevant suites.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_suites: Vec<String>,
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        self.untracked_calls.is_empty()
            && self.untracked_records.is_empty()
            && self.unanalyzed.is_empty()
            && self.suggested_suites.is_empty()
    }
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
