// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Patch<S>` typestate pipeline plus its public data types.
//!
//! The unified-diff parser lives in `compat::diff`; the per-pin evaluator
//! lives in `compat::evaluate`. This module is the entry point and the
//! type-state glue.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::compat::arg_shape::ArgShape;
use crate::compat::call_sites::{
    AttrCtxScanner, DynamicCall, extract_call_args_into, extract_definitions_into,
    extract_dynamic_into, extract_into_with_macros, extract_type_refs_into, strip_line_comment,
};
use crate::compat::diff;
use crate::compat::evaluate::{PostimageTally, evaluate_pin};
use crate::compat::patch_facts::{
    classify as classify_patch_facts, is_only_test_visibility_change, suggest_suites,
};
use crate::compat::scope::{PinScope, UntrackedTally};
use crate::config::FamilyDefaults;
use crate::erlang_macros::MacroTable;
use crate::errors::PatchError;
use crate::model::names::{Mfa, ModuleName, ProjectName, RecordName, RelativePath};
use crate::model::pin::Pin;
use crate::model::snapshot::{Snapshot, state};
use crate::model::symbol::{RefContext, RefOrigin, SymbolKind, SymbolRef};
use crate::model::verdict::{
    AlreadyPresent, ContentPresence, Diagnostics, PinVerdict, SeriesEvaluation, SeriesVerdict,
    Unanalyzed,
};

pub use crate::compat::diff::PATCH_SIZE_LIMIT;

pub mod patch_state {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Raw;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Analyzed;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Verdicted;
}

pub use patch_state::{Analyzed, Raw, Verdicted};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Language {
    Erlang,
    Elixir,
    /// Cuttlefish `.schema`, `.snippets`, and `.partial` files: Erlang
    /// fragments embedded in a config-key DSL. We run the call-site
    /// extractor over every line so MFA references inside
    /// `{translation, ..., fun(Conf) -> M:F(...) end}` blocks land in
    /// `Patch.referenced`. The `.partial` extension was introduced in
    /// cuttlefish 3.8.0.
    CuttlefishSchema,
    Other,
}

impl Language {
    pub fn from_path(path: &str) -> Self {
        if path.ends_with(".erl") || path.ends_with(".hrl") {
            Self::Erlang
        } else if path.ends_with(".ex") || path.ends_with(".exs") {
            Self::Elixir
        } else if path.ends_with(".schema")
            || path.ends_with(".snippets")
            || path.ends_with(".partial")
        {
            Self::CuttlefishSchema
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchedFile {
    pub old_path: Option<PathBuf>,
    pub new_path: Option<PathBuf>,
    pub language: Language,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    Context(String),
    Added(String),
    Removed(String),
}

impl HunkLine {
    pub fn is_addition(&self) -> bool {
        matches!(self, Self::Added(_))
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Context(s) | Self::Added(s) | Self::Removed(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch<S = Raw> {
    pub files: Vec<PatchedFile>,
    referenced: Vec<SymbolRef>,
    defined: Vec<SymbolRef>,
    dynamic_calls: Vec<DynamicCall>,
    unsupported_files: Vec<PathBuf>,
    call_args: Vec<(Mfa, Vec<ArgShape>)>,
    verdicts: Vec<PinVerdict>,
    _state: PhantomData<S>,
}

impl Patch<Raw> {
    pub fn parse(input: &[u8]) -> Result<Self, PatchError> {
        let files = diff::parse(input)?;
        Ok(Self {
            files,
            referenced: Vec::new(),
            defined: Vec::new(),
            dynamic_calls: Vec::new(),
            unsupported_files: Vec::new(),
            call_args: Vec::new(),
            verdicts: Vec::new(),
            _state: PhantomData,
        })
    }

    /// Run the per-file analyzer with no macro context. Equivalent to
    /// `analyze_with_macros(&BTreeMap::new())`.
    #[must_use]
    pub fn analyze(self) -> Patch<Analyzed> {
        self.analyze_with_macros(&BTreeMap::new())
    }

    /// Run the per-file analyzer. Each `PatchedFile` already carries a
    /// `Language` tag from the diff parser; the analyzer dispatches per
    /// file. Erlang files run through the call-site extractor; Elixir
    /// files are recorded as unsupported so the evaluator can emit
    /// `Reason::UnsupportedFileType` rather than silently flagging the
    /// patch as `Compatible`. Files with `Language::Other` (build
    /// scripts, markdown, etc.) are still silent-skipped.
    ///
    /// `macros_by_path` supplies a per-file macro table built from the
    /// source pin's content (including resolved `-include` chains).
    /// Calls hidden behind `?Name` and `?Name:f(args)` are resolved
    /// against the matching file's table; apply-family BIFs use it to
    /// resolve macro-module arguments.
    #[must_use]
    pub fn analyze_with_macros(
        self,
        macros_by_path: &BTreeMap<PathBuf, MacroTable>,
    ) -> Patch<Analyzed> {
        let mut referenced: Vec<SymbolRef> = Vec::new();
        let mut defined: Vec<SymbolRef> = Vec::new();
        let mut dynamic_calls: Vec<DynamicCall> = Vec::new();
        let mut unsupported_files: Vec<PathBuf> = Vec::new();
        let mut call_args: Vec<(Mfa, Vec<ArgShape>)> = Vec::new();
        let empty = MacroTable::new();
        for file in &self.files {
            if file.binary {
                continue;
            }
            let file_macros = match (&file.new_path, &file.old_path) {
                (Some(p), _) | (None, Some(p)) => macros_by_path.get(p).unwrap_or(&empty),
                (None, None) => &empty,
            };
            match file.language {
                Language::Erlang => {
                    for hunk in &file.hunks {
                        let mut scanner = AttrCtxScanner::new();
                        for line in &hunk.lines {
                            let (text, origin) = match line {
                                HunkLine::Added(text) => (text, RefOrigin::Added),
                                HunkLine::Context(text) => (text, RefOrigin::Context),
                                HunkLine::Removed(_) => continue,
                            };
                            // Strip once; the extractors' own strips no-op on
                            // an already-stripped line.
                            let text = strip_line_comment(text);
                            let mut line_refs: Vec<SymbolRef> = Vec::new();
                            match scanner.classify(text) {
                                RefContext::TypeAttribute => {
                                    extract_type_refs_into(text, &mut line_refs);
                                }
                                RefContext::Body => {
                                    extract_into_with_macros(text, file_macros, &mut line_refs);
                                    extract_definitions_into(text, &mut defined);
                                    extract_dynamic_into(text, &mut dynamic_calls);
                                    // Context-line call shapes are pre-existing
                                    // target facts; only added calls feed the
                                    // clause-mismatch comparison.
                                    if origin == RefOrigin::Added {
                                        extract_call_args_into(text, &mut call_args);
                                    }
                                }
                            }
                            for r in line_refs {
                                referenced.push(r.with_origin(origin));
                            }
                        }
                    }
                }
                Language::CuttlefishSchema => {
                    for hunk in &file.hunks {
                        for line in &hunk.lines {
                            let (text, origin) = match line {
                                HunkLine::Added(text) => (text, RefOrigin::Added),
                                HunkLine::Context(text) => (text, RefOrigin::Context),
                                HunkLine::Removed(_) => continue,
                            };
                            let mut line_refs: Vec<SymbolRef> = Vec::new();
                            extract_into_with_macros(text, file_macros, &mut line_refs);
                            for r in line_refs {
                                referenced.push(r.with_origin(origin));
                            }
                        }
                    }
                }
                Language::Elixir => {
                    if let Some(path) = file.new_path.as_ref().or(file.old_path.as_ref()) {
                        unsupported_files.push(path.clone());
                    }
                }
                Language::Other => {}
            }
        }
        referenced.sort();
        // `Added` sorts before `Context` within a kind, so a kind-level
        // dedup keeps the strongest origin.
        referenced.dedup_by(|a, b| a.kind == b.kind);
        defined.sort();
        defined.dedup();
        unsupported_files.sort();
        unsupported_files.dedup();
        Patch {
            files: self.files,
            referenced,
            defined,
            dynamic_calls,
            unsupported_files,
            call_args,
            verdicts: Vec::new(),
            _state: PhantomData,
        }
    }
}

/// One pin's filesystem state at evaluation time. Holds the content of
/// every path the patch touches, or `None` for paths absent at that pin.
/// Built by the caller (the CLI uses `gix`); the analyzer itself never
/// knows about git.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationFiles {
    files: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl EvaluationFiles {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, path: impl Into<PathBuf>, contents: Option<Vec<u8>>) -> Self {
        self.files.insert(path.into(), contents);
        self
    }

    pub fn get(&self, path: &Path) -> Option<&Option<Vec<u8>>> {
        self.files.get(path)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
}

pub mod evaluation_state {
    use super::{EvaluationFiles, PinScope};

    /// State markers carry the data they describe: `Scoped` carries the
    /// scope, `Sourced` carries scope plus files. This is how we avoid
    /// `Option<PinScope>` + runtime `.expect` panics: the type system
    /// proves field presence at the call site.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Pinned;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Scoped {
        pub(super) scope: PinScope,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Sourced {
        pub(super) scope: PinScope,
        pub(super) files: EvaluationFiles,
    }
}

pub use evaluation_state::{Pinned, Scoped, Sourced};

/// Inputs for one pin's evaluation, lifted to a type-state pipeline so
/// half-built contexts cannot reach the evaluator. The progression is
/// `Pinned` (identity) → `Scoped` (added module/record scope) → `Sourced`
/// (added file bytes for context-drift checks). Only `Scoped` and
/// `Sourced` contexts can be evaluated; the `EvaluationInput` trait lets
/// the evaluator accept either. Per-state data lives inside the marker
/// (`extras`), so there are no `Option` fields and no runtime `expect`s.
///
/// `source_snapshot` is optional regardless of state: it carries the
/// matching snapshot at the *source* pin (the version the patch was
/// authored against), enabling spec and record-field diffs that flag
/// signature narrowing between source and target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationContext<S = Scoped> {
    pin: Pin,
    // Arc: batch evaluation reuses one parsed snapshot across
    // hundreds of (commit, series) pairs without deep clones
    snapshot: Arc<Snapshot<state::Canonical>>,
    source_snapshot: Option<Arc<Snapshot<state::Canonical>>>,
    family_defaults: Option<FamilyDefaults>,
    extras: S,
}

impl EvaluationContext<Pinned> {
    pub fn for_pin(pin: Pin, snapshot: impl Into<Arc<Snapshot<state::Canonical>>>) -> Self {
        Self {
            pin,
            snapshot: snapshot.into(),
            source_snapshot: None,
            family_defaults: None,
            extras: Pinned,
        }
    }

    #[must_use]
    pub fn with_scope(self, scope: PinScope) -> EvaluationContext<Scoped> {
        EvaluationContext {
            pin: self.pin,
            snapshot: self.snapshot,
            source_snapshot: self.source_snapshot,
            family_defaults: self.family_defaults,
            extras: Scoped { scope },
        }
    }
}

impl EvaluationContext<Scoped> {
    /// Shortcut for `EvaluationContext::for_pin(pin, snap).with_scope(scope)`.
    pub fn new(
        pin: Pin,
        snapshot: impl Into<Arc<Snapshot<state::Canonical>>>,
        scope: PinScope,
    ) -> Self {
        EvaluationContext::for_pin(pin, snapshot).with_scope(scope)
    }

    #[must_use]
    pub fn with_files(self, files: EvaluationFiles) -> EvaluationContext<Sourced> {
        EvaluationContext {
            pin: self.pin,
            snapshot: self.snapshot,
            source_snapshot: self.source_snapshot,
            family_defaults: self.family_defaults,
            extras: Sourced {
                scope: self.extras.scope,
                files,
            },
        }
    }
}

impl<S> EvaluationContext<S> {
    #[must_use]
    pub fn with_source_snapshot(
        mut self,
        source: impl Into<Arc<Snapshot<state::Canonical>>>,
    ) -> Self {
        self.source_snapshot = Some(source.into());
        self
    }

    /// Attach the pin's project-family declarations. Contexts without
    /// this skip the versioned-machine and wire-constant checks.
    #[must_use]
    pub fn with_family_defaults(mut self, defaults: FamilyDefaults) -> Self {
        self.family_defaults = Some(defaults);
        self
    }
}

/// Anything the evaluator can read: a context that has at least a scope.
/// Implemented for `EvaluationContext<Scoped>` and `EvaluationContext<Sourced>`.
pub trait EvaluationInput {
    fn pin(&self) -> &Pin;
    fn snapshot(&self) -> &Snapshot<state::Canonical>;
    fn scope(&self) -> &PinScope;
    fn files(&self) -> Option<&EvaluationFiles>;
    fn source_snapshot(&self) -> Option<&Snapshot<state::Canonical>>;
    fn family_defaults(&self) -> Option<&FamilyDefaults>;
}

impl EvaluationInput for EvaluationContext<Scoped> {
    fn pin(&self) -> &Pin {
        &self.pin
    }
    fn snapshot(&self) -> &Snapshot<state::Canonical> {
        &self.snapshot
    }
    fn scope(&self) -> &PinScope {
        &self.extras.scope
    }
    fn files(&self) -> Option<&EvaluationFiles> {
        None
    }
    fn source_snapshot(&self) -> Option<&Snapshot<state::Canonical>> {
        self.source_snapshot.as_deref()
    }
    fn family_defaults(&self) -> Option<&FamilyDefaults> {
        self.family_defaults.as_ref()
    }
}

impl EvaluationInput for EvaluationContext<Sourced> {
    fn pin(&self) -> &Pin {
        &self.pin
    }
    fn snapshot(&self) -> &Snapshot<state::Canonical> {
        &self.snapshot
    }
    fn scope(&self) -> &PinScope {
        &self.extras.scope
    }
    fn files(&self) -> Option<&EvaluationFiles> {
        Some(&self.extras.files)
    }
    fn source_snapshot(&self) -> Option<&Snapshot<state::Canonical>> {
        self.source_snapshot.as_deref()
    }
    fn family_defaults(&self) -> Option<&FamilyDefaults> {
        self.family_defaults.as_ref()
    }
}

impl Patch<Analyzed> {
    pub fn referenced(&self) -> &[SymbolRef] {
        &self.referenced
    }

    pub fn defined(&self) -> &[SymbolRef] {
        &self.defined
    }

    /// True when every changed `.erl` hunk is a Variant A unwrap of
    /// test-only `-ifdef(TEST)` machinery (see
    /// `InapplicableReason::OnlyTestVisibilityChanged`). Callers stamp
    /// the result onto each `PinVerdict.touched.only_test_visibility`
    /// so the inapplicable promoter can flip the verdict.
    pub fn is_only_test_visibility_change(&self) -> bool {
        is_only_test_visibility_change(&self.files)
    }

    pub fn against(self, snapshot: &Snapshot<state::Canonical>, pin: Pin) -> Patch<Verdicted> {
        let result = evaluate_pin(
            &self.files,
            &self.referenced,
            &self.defined,
            &self.unsupported_files,
            &self.call_args,
            snapshot,
            None,
            None,
            None,
            None,
        );
        let mut verdicts = self.verdicts;
        verdicts.push(PinVerdict::new(pin, result.verdict));
        Patch {
            files: self.files,
            referenced: self.referenced,
            defined: self.defined,
            dynamic_calls: self.dynamic_calls,
            unsupported_files: self.unsupported_files,
            call_args: self.call_args,
            verdicts,
            _state: PhantomData,
        }
    }

    pub fn against_series(self, snapshots: &[(Pin, Snapshot<state::Canonical>)]) -> SeriesVerdict {
        let mut results = Vec::with_capacity(snapshots.len());
        for (pin, snap) in snapshots {
            let r = evaluate_pin(
                &self.files,
                &self.referenced,
                &self.defined,
                &self.unsupported_files,
                &self.call_args,
                snap,
                None,
                None,
                None,
                None,
            );
            results.push(PinVerdict::new(pin.clone(), r.verdict));
        }
        SeriesVerdict::from_results(results)
    }

    pub fn against_series_with_files(
        self,
        snapshots: &[(Pin, Snapshot<state::Canonical>, EvaluationFiles)],
    ) -> SeriesVerdict {
        let mut results = Vec::with_capacity(snapshots.len());
        for (pin, snap, files) in snapshots {
            let r = evaluate_pin(
                &self.files,
                &self.referenced,
                &self.defined,
                &self.unsupported_files,
                &self.call_args,
                snap,
                None,
                Some(files),
                None,
                None,
            );
            results.push(PinVerdict::new(pin.clone(), r.verdict));
        }
        SeriesVerdict::from_results(results)
    }

    /// Scope-aware evaluation. Out-of-scope call sites and record
    /// references are skipped per pin and tallied series-wide into a
    /// `Diagnostics` envelope kept strictly outside `Verdict.reasons`.
    /// Generic over `EvaluationInput` so callers pass `Scoped` (no file
    /// checks) or `Sourced` (with file-context checks) contexts.
    pub fn evaluate_series<C: EvaluationInput>(self, contexts: &[C]) -> SeriesEvaluation {
        let series_modules: BTreeSet<ModuleName> = contexts
            .iter()
            .flat_map(|c| c.scope().modules().iter().cloned())
            .collect();
        let series_records: BTreeSet<RecordName> = contexts
            .iter()
            .flat_map(|c| c.scope().records().iter().cloned())
            .collect();
        let mut untracked_calls = UntrackedTally::default();
        let mut untracked_records: BTreeMap<RecordName, usize> = BTreeMap::new();
        let defined_index: HashSet<&SymbolKind> = self.defined.iter().map(|d| &d.kind).collect();
        for r in &self.referenced {
            if defined_index.contains(&r.kind) {
                continue;
            }
            match &r.kind {
                SymbolKind::Function { mfa } if !series_modules.contains(&mfa.module) => {
                    untracked_calls.record(mfa.module.clone());
                }
                SymbolKind::FunctionAnyArity { module, .. } if !series_modules.contains(module) => {
                    untracked_calls.record(module.clone());
                }
                SymbolKind::Record { name } if !series_records.contains(name) => {
                    *untracked_records.entry(name.clone()).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        let mut results = Vec::with_capacity(contexts.len());
        let mut context_refs_missing: BTreeMap<ModuleName, usize> = BTreeMap::new();
        // One pin's tally becomes the series-level content-presence
        // diagnostic: the one that considered the most hunks, which
        // in cross-branch mode is the self pin carrying target bytes.
        let mut best_presence: Option<ContentPresence> = None;
        for ctx in contexts {
            let r = evaluate_pin(
                &self.files,
                &self.referenced,
                &self.defined,
                &self.unsupported_files,
                &self.call_args,
                ctx.snapshot(),
                ctx.source_snapshot(),
                ctx.files(),
                Some(ctx.scope()),
                ctx.family_defaults(),
            );
            if let Some(tally) = r.postimage {
                let denser = best_presence
                    .as_ref()
                    .is_none_or(|b| tally.considered > b.hunks_considered);
                if denser {
                    best_presence = Some(content_presence_from_tally(
                        ctx.pin().project.clone(),
                        tally,
                    ));
                }
            }
            // Pin scopes are disjoint per project, so summing per-pin
            // tallies does not double count a module.
            for (module, count) in r.context_missing {
                *context_refs_missing.entry(module).or_insert(0) += count;
            }
            results.push(
                PinVerdict::new(ctx.pin().clone(), r.verdict)
                    .with_tracked_ref_details(r.tracked_refs)
                    .with_source_delta_details(r.source_deltas),
            );
        }
        let mut unanalyzed = Unanalyzed::default();
        for d in &self.dynamic_calls {
            match d {
                DynamicCall::Apply => unanalyzed.apply += 1,
                DynamicCall::VariableDispatch => unanalyzed.variable_dispatch += 1,
            }
        }
        let already_present = best_presence.map(|content| AlreadyPresent {
            identical: None,
            content: Some(content),
        });
        SeriesEvaluation {
            verdict: SeriesVerdict::from_results(results),
            diagnostics: Diagnostics {
                untracked_calls: untracked_calls.into_map(),
                untracked_records,
                context_refs_missing,
                unattributed_paths: BTreeMap::new(),
                unanalyzed,
                suggested_suites: suggest_suites(&self.files),
                missing_test_modules: BTreeMap::new(),
                already_present,
                already_present_skipped: None,
                dep_pin_divergence: Vec::new(),
                pin_bumps: Vec::new(),
            },
            patch_facts: classify_patch_facts(&self.files),
            touched_paths: collect_touched_paths(&self.files),
            pr_commits: None,
        }
    }
}

/// Iterate `PatchedFile`s in diff order, project to `RelativePath`,
/// dedup by first occurrence. Paths that can't be expressed as UTF-8
/// relative paths (extremely rare in practice; never seen in RabbitMQ)
/// are dropped silently: the alternative is failing the verdict.
/// Project a per-pin tally into the envelope shape, converting the
/// per-file keys to `RelativePath` with the same drop-on-failure
/// policy as `collect_touched_paths`.
fn content_presence_from_tally(pin: ProjectName, tally: PostimageTally) -> ContentPresence {
    let mut per_file = BTreeMap::new();
    for (path, counts) in tally.per_file {
        let Some(s) = path.to_str() else { continue };
        let Ok(rel) = RelativePath::new(s.to_owned()) else {
            continue;
        };
        per_file.insert(rel, counts);
    }
    ContentPresence {
        pin,
        hunks_already_applied: tally.applied,
        hunks_considered: tally.considered,
        hunks_ambiguous: tally.ambiguous,
        hunks_low_confidence: tally.low_confidence,
        per_file,
    }
}

fn collect_touched_paths(files: &[PatchedFile]) -> Vec<RelativePath> {
    let mut seen: BTreeSet<RelativePath> = BTreeSet::new();
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        for p in [file.new_path.as_ref(), file.old_path.as_ref()]
            .into_iter()
            .flatten()
        {
            let Some(s) = p.to_str() else { continue };
            let Ok(rel) = RelativePath::new(s.to_owned()) else {
                continue;
            };
            if seen.insert(rel.clone()) {
                out.push(rel);
            }
        }
    }
    out
}

impl Patch<Verdicted> {
    pub fn verdicts(&self) -> &[PinVerdict] {
        &self.verdicts
    }
}
