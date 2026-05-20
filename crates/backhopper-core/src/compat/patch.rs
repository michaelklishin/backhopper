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

use crate::compat::arg_shape::ArgShape;
use crate::compat::call_sites::{
    DynamicCall, extract_call_args_into, extract_definitions_into, extract_dynamic_into,
    extract_into_with_macros,
};
use crate::compat::diff;
use crate::compat::evaluate::evaluate_pin;
use crate::compat::scope::{PinScope, UntrackedTally};
use crate::erlang_macros::MacroTable;
use crate::errors::PatchError;
use crate::model::names::{Mfa, ModuleName, RecordName};
use crate::model::pin::Pin;
use crate::model::snapshot::{Snapshot, state};
use crate::model::symbol::{SymbolKind, SymbolRef};
use crate::model::verdict::{Diagnostics, PinVerdict, SeriesEvaluation, SeriesVerdict, Unanalyzed};

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
    Other,
}

impl Language {
    pub fn from_path(path: &str) -> Self {
        if path.ends_with(".erl") || path.ends_with(".hrl") {
            Self::Erlang
        } else if path.ends_with(".ex") || path.ends_with(".exs") {
            Self::Elixir
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
                        for line in &hunk.lines {
                            match line {
                                HunkLine::Added(text) | HunkLine::Context(text) => {
                                    extract_into_with_macros(text, file_macros, &mut referenced);
                                    extract_definitions_into(text, &mut defined);
                                    extract_dynamic_into(text, &mut dynamic_calls);
                                    extract_call_args_into(text, &mut call_args);
                                }
                                HunkLine::Removed(_) => {}
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
        referenced.dedup();
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
    snapshot: Snapshot<state::Canonical>,
    source_snapshot: Option<Snapshot<state::Canonical>>,
    extras: S,
}

impl EvaluationContext<Pinned> {
    pub fn for_pin(pin: Pin, snapshot: Snapshot<state::Canonical>) -> Self {
        Self {
            pin,
            snapshot,
            source_snapshot: None,
            extras: Pinned,
        }
    }

    #[must_use]
    pub fn with_scope(self, scope: PinScope) -> EvaluationContext<Scoped> {
        EvaluationContext {
            pin: self.pin,
            snapshot: self.snapshot,
            source_snapshot: self.source_snapshot,
            extras: Scoped { scope },
        }
    }
}

impl EvaluationContext<Scoped> {
    /// Shortcut for `EvaluationContext::for_pin(pin, snap).with_scope(scope)`.
    pub fn new(pin: Pin, snapshot: Snapshot<state::Canonical>, scope: PinScope) -> Self {
        EvaluationContext::for_pin(pin, snapshot).with_scope(scope)
    }

    #[must_use]
    pub fn with_files(self, files: EvaluationFiles) -> EvaluationContext<Sourced> {
        EvaluationContext {
            pin: self.pin,
            snapshot: self.snapshot,
            source_snapshot: self.source_snapshot,
            extras: Sourced {
                scope: self.extras.scope,
                files,
            },
        }
    }
}

impl<S> EvaluationContext<S> {
    #[must_use]
    pub fn with_source_snapshot(mut self, source: Snapshot<state::Canonical>) -> Self {
        self.source_snapshot = Some(source);
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
        self.source_snapshot.as_ref()
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
        self.source_snapshot.as_ref()
    }
}

impl Patch<Analyzed> {
    pub fn referenced(&self) -> &[SymbolRef] {
        &self.referenced
    }

    pub fn defined(&self) -> &[SymbolRef] {
        &self.defined
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
        let defined_index: HashSet<&SymbolRef> = self.defined.iter().collect();
        for r in &self.referenced {
            if defined_index.contains(r) {
                continue;
            }
            match &r.kind {
                SymbolKind::Function { mfa } if !series_modules.contains(&mfa.module) => {
                    untracked_calls.record(mfa.module.clone());
                }
                SymbolKind::Record { name } if !series_records.contains(name) => {
                    *untracked_records.entry(name.clone()).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        let mut results = Vec::with_capacity(contexts.len());
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
            );
            results.push(
                PinVerdict::new(ctx.pin().clone(), r.verdict)
                    .with_tracked_ref_details(r.tracked_refs),
            );
        }
        let mut unanalyzed = Unanalyzed::default();
        for d in &self.dynamic_calls {
            match d {
                DynamicCall::Apply => unanalyzed.apply += 1,
                DynamicCall::VariableDispatch => unanalyzed.variable_dispatch += 1,
            }
        }
        SeriesEvaluation {
            verdict: SeriesVerdict::from_results(results),
            diagnostics: Diagnostics {
                untracked_calls: untracked_calls.into_map(),
                untracked_records,
                unanalyzed,
            },
        }
    }
}

impl Patch<Verdicted> {
    pub fn verdicts(&self) -> &[PinVerdict] {
        &self.verdicts
    }
}