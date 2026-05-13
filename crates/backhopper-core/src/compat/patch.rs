//! Unified-diff parser plus `Patch<S>` typestate pipeline.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str;

use crate::compat::call_sites::{
    DynamicCall, extract_definitions_into, extract_dynamic_into, extract_into,
};
use crate::compat::scope::{PinScope, UntrackedTally};
use crate::errors::PatchError;
use crate::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName};
use crate::model::pin::Pin;
use crate::model::snapshot::{ArityMatch, Snapshot, Visibility, state};
use crate::model::symbol::{SymbolKind, SymbolRef};
use crate::model::verdict::{
    Diagnostics, PinVerdict, Reason, SeriesEvaluation, SeriesVerdict, Unanalyzed, Verdict,
};

pub const PATCH_SIZE_LIMIT: usize = 64 * 1024 * 1024;

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
    verdicts: Vec<PinVerdict>,
    _state: PhantomData<S>,
}

impl Patch<Raw> {
    pub fn parse(input: &[u8]) -> Result<Self, PatchError> {
        if input.len() > PATCH_SIZE_LIMIT {
            return Err(PatchError::SizeLimit {
                size: input.len(),
                limit: PATCH_SIZE_LIMIT,
            });
        }
        let text = str::from_utf8(input)
            .unwrap_or_else(|_| str::from_utf8(&input[..input.len().min(64)]).unwrap_or(""));
        let files = parse_unified_diff(text)?;
        Ok(Self {
            files,
            referenced: Vec::new(),
            defined: Vec::new(),
            dynamic_calls: Vec::new(),
            verdicts: Vec::new(),
            _state: PhantomData,
        })
    }

    pub fn analyze(self, _language: Language) -> Patch<Analyzed> {
        let mut referenced: Vec<SymbolRef> = Vec::new();
        let mut defined: Vec<SymbolRef> = Vec::new();
        let mut dynamic_calls: Vec<DynamicCall> = Vec::new();
        for file in &self.files {
            if file.binary || file.language != Language::Erlang {
                continue;
            }
            for hunk in &file.hunks {
                for line in &hunk.lines {
                    match line {
                        HunkLine::Added(text) | HunkLine::Context(text) => {
                            extract_into(text, &mut referenced);
                            extract_definitions_into(text, &mut defined);
                            extract_dynamic_into(text, &mut dynamic_calls);
                        }
                        HunkLine::Removed(_) => {}
                    }
                }
            }
        }
        referenced.sort_by_key(symbol_sort_key);
        referenced.dedup();
        defined.sort_by_key(symbol_sort_key);
        defined.dedup();
        Patch {
            files: self.files,
            referenced,
            defined,
            dynamic_calls,
            verdicts: Vec::new(),
            _state: PhantomData,
        }
    }
}

/// A view of one pin's filesystem state at evaluation time. Holds the
/// content of every path the patch touches, or `None` for paths absent
/// at that pin. Built by the caller (the CLI uses `gix`); the analyzer
/// itself never knows about git.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinFiles {
    files: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl PinFiles {
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

/// Inputs for one pin's evaluation: identity, snapshot, optional files, scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinContext {
    pub pin: Pin,
    pub snapshot: Snapshot<state::Canonical>,
    pub files: Option<PinFiles>,
    pub scope: PinScope,
}

impl PinContext {
    pub fn new(pin: Pin, snapshot: Snapshot<state::Canonical>, scope: PinScope) -> Self {
        Self {
            pin,
            snapshot,
            files: None,
            scope,
        }
    }

    #[must_use]
    pub fn with_files(mut self, files: PinFiles) -> Self {
        self.files = Some(files);
        self
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
        let (verdict, _) = evaluate_pin(
            &self.files,
            &self.referenced,
            &self.defined,
            snapshot,
            None,
            None,
        );
        let mut verdicts = self.verdicts.clone();
        verdicts.push(PinVerdict::new(pin, verdict));
        Patch {
            files: self.files,
            referenced: self.referenced,
            defined: self.defined,
            dynamic_calls: self.dynamic_calls,
            verdicts,
            _state: PhantomData,
        }
    }

    pub fn against_series(self, snapshots: &[(Pin, Snapshot<state::Canonical>)]) -> SeriesVerdict {
        let mut results = Vec::with_capacity(snapshots.len());
        for (pin, snap) in snapshots {
            let (verdict, _) = evaluate_pin(
                &self.files,
                &self.referenced,
                &self.defined,
                snap,
                None,
                None,
            );
            results.push(PinVerdict::new(pin.clone(), verdict));
        }
        SeriesVerdict::from_results(results)
    }

    pub fn against_series_with_files(
        self,
        snapshots: &[(Pin, Snapshot<state::Canonical>, PinFiles)],
    ) -> SeriesVerdict {
        let mut results = Vec::with_capacity(snapshots.len());
        for (pin, snap, files) in snapshots {
            let (verdict, _) = evaluate_pin(
                &self.files,
                &self.referenced,
                &self.defined,
                snap,
                Some(files),
                None,
            );
            results.push(PinVerdict::new(pin.clone(), verdict));
        }
        SeriesVerdict::from_results(results)
    }

    /// Scope-aware evaluation. Out-of-scope call sites and record
    /// references are skipped per pin and tallied series-wide into a
    /// `Diagnostics` envelope kept strictly outside `Verdict.reasons`.
    pub fn evaluate_series(self, contexts: &[PinContext]) -> SeriesEvaluation {
        let series_modules: BTreeSet<ModuleName> = contexts
            .iter()
            .flat_map(|c| c.scope.modules().iter().cloned())
            .collect();
        let series_records: BTreeSet<RecordName> = contexts
            .iter()
            .flat_map(|c| c.scope.records().iter().cloned())
            .collect();
        let mut untracked_calls = UntrackedTally::default();
        let mut untracked_records: BTreeMap<RecordName, usize> = BTreeMap::new();
        for r in &self.referenced {
            if self.defined.contains(r) {
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
            let (verdict, tracked_refs) = evaluate_pin(
                &self.files,
                &self.referenced,
                &self.defined,
                &ctx.snapshot,
                ctx.files.as_ref(),
                Some(&ctx.scope),
            );
            results.push(PinVerdict::new(ctx.pin.clone(), verdict).with_tracked_refs(tracked_refs));
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

fn evaluate_pin(
    files: &[PatchedFile],
    referenced: &[SymbolRef],
    defined: &[SymbolRef],
    snapshot: &Snapshot<state::Canonical>,
    pin_files: Option<&PinFiles>,
    scope: Option<&PinScope>,
) -> (Verdict, usize) {
    let mut reasons: Vec<Reason> = Vec::new();
    if let Some(pf) = pin_files {
        check_files_against_pin(files, pf, &mut reasons);
    }
    let mut tracked_refs = 0usize;
    for r in referenced {
        if defined.contains(r) {
            continue;
        }
        match &r.kind {
            SymbolKind::Function { mfa } => {
                if let Some(s) = scope
                    && !s.contains_module(&mfa.module)
                {
                    continue;
                }
                tracked_refs += 1;
                analyze_function_reference(r, mfa, snapshot, &mut reasons);
            }
            SymbolKind::Record { name } => {
                if let Some(s) = scope
                    && !s.contains_record(name)
                {
                    continue;
                }
                if !record_present(snapshot, name) {
                    reasons.push(Reason::MissingSymbol {
                        symbol: r.clone(),
                        first_seen_at_tag: None,
                        suggested_replacement: None,
                    });
                }
            }
            _ => {}
        }
    }
    (Verdict::from_reasons(reasons), tracked_refs)
}

fn check_files_against_pin(files: &[PatchedFile], pin_files: &PinFiles, reasons: &mut Vec<Reason>) {
    for file in files {
        let path = match (&file.new_path, &file.old_path) {
            (Some(p), _) | (None, Some(p)) => p.clone(),
            (None, None) => continue,
        };
        let Some(slot) = pin_files.get(&path) else {
            continue;
        };
        let Some(bytes) = slot else {
            reasons.push(Reason::FileAbsent { path: path.clone() });
            continue;
        };
        let target_lines = match str::from_utf8(bytes) {
            Ok(s) => s.lines().collect::<Vec<&str>>(),
            Err(_) => continue,
        };
        for (idx, hunk) in file.hunks.iter().enumerate() {
            if !hunk_context_matches(hunk, &target_lines) {
                reasons.push(Reason::ContextDrift {
                    path: path.clone(),
                    hunk_index: idx,
                });
            }
        }
    }
}

fn hunk_context_matches(hunk: &Hunk, target_lines: &[&str]) -> bool {
    let mut row = hunk.old_start.saturating_sub(1);
    for line in &hunk.lines {
        match line {
            HunkLine::Added(_) => {}
            HunkLine::Context(text) | HunkLine::Removed(text) => {
                let Some(actual) = target_lines.get(row) else {
                    return false;
                };
                if *actual != text.as_str() {
                    return false;
                }
                row += 1;
            }
        }
    }
    true
}

fn analyze_function_reference(
    r: &SymbolRef,
    mfa: &Mfa,
    snapshot: &Snapshot<state::Canonical>,
    reasons: &mut Vec<Reason>,
) {
    if let Some(module) = snapshot.modules().iter().find(|m| m.name == mfa.module)
        && module.visibility == Visibility::Hidden
    {
        reasons.push(Reason::NowHidden {
            module: mfa.module.clone(),
        });
        return;
    }
    if is_function_deprecated(snapshot, mfa) {
        reasons.push(Reason::DeprecatedUsage {
            symbol: r.clone(),
            since: None,
            replacement: None,
        });
    }
    if !snapshot.lookup_export(&mfa.module, &mfa.function, mfa.arity) {
        let alt_arities = collect_alt_arities(snapshot, &mfa.module, &mfa.function);
        if !alt_arities.is_empty() && !alt_arities.contains(&mfa.arity) {
            reasons.push(Reason::ArityChanged {
                module: mfa.module.clone(),
                function: mfa.function.clone(),
                expected: mfa.arity,
                found: alt_arities,
            });
        } else {
            reasons.push(Reason::MissingSymbol {
                symbol: r.clone(),
                first_seen_at_tag: None,
                suggested_replacement: None,
            });
        }
    }
}

fn is_function_deprecated(snapshot: &Snapshot<state::Canonical>, mfa: &Mfa) -> bool {
    let Some(module) = snapshot.modules().iter().find(|m| m.name == mfa.module) else {
        return false;
    };
    for d in &module.deprecations {
        if d.module_wide {
            return true;
        }
        if let Some(f) = &d.function
            && f == &mfa.function
        {
            match d.arity_match {
                ArityMatch::Any => return true,
                ArityMatch::Exact { arity } if arity == mfa.arity => return true,
                ArityMatch::Exact { .. } => {}
            }
        }
    }
    false
}

fn record_present(snapshot: &Snapshot<state::Canonical>, name: &RecordName) -> bool {
    snapshot
        .headers()
        .iter()
        .any(|h| h.records.iter().any(|r| &r.name == name))
}

fn collect_alt_arities(
    snapshot: &Snapshot<state::Canonical>,
    module: &ModuleName,
    function: &FunctionName,
) -> Vec<Arity> {
    snapshot
        .modules()
        .iter()
        .find(|m| &m.name == module)
        .map(|m| {
            m.exports
                .iter()
                .filter(|fa| &fa.name == function)
                .map(|fa| fa.arity)
                .collect()
        })
        .unwrap_or_default()
}

fn symbol_sort_key(s: &SymbolRef) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn parse_unified_diff(text: &str) -> Result<Vec<PatchedFile>, PatchError> {
    let mut files: Vec<PatchedFile> = Vec::new();
    let mut current: Option<PatchedFile> = None;
    let mut current_hunk: Option<Hunk> = None;
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if line.starts_with("diff --git ") {
            if let Some(mut f) = current.take() {
                if let Some(h) = current_hunk.take() {
                    f.hunks.push(h);
                }
                files.push(f);
            }
            current = Some(PatchedFile {
                old_path: None,
                new_path: None,
                language: Language::Other,
                binary: false,
                hunks: Vec::new(),
            });
        } else if line.starts_with("--- ") {
            if let Some(f) = current.as_mut() {
                f.old_path = parse_diff_path(&line[4..]);
            }
        } else if line.starts_with("+++ ") {
            if let Some(f) = current.as_mut() {
                let path = parse_diff_path(&line[4..]);
                if let Some(p) = &path {
                    f.language = Language::from_path(&p.to_string_lossy());
                }
                f.new_path = path;
            }
        } else if line.starts_with("Binary files") || line.contains("GIT binary patch") {
            if let Some(f) = current.as_mut() {
                f.binary = true;
            }
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(f) = current.as_mut() {
                if let Some(h) = current_hunk.take() {
                    f.hunks.push(h);
                }
                let header = parse_hunk_header(rest, lineno)?;
                current_hunk = Some(header);
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            push_line(&mut current_hunk, HunkLine::Added(line[1..].to_owned()));
        } else if line.starts_with('-') && !line.starts_with("---") {
            push_line(&mut current_hunk, HunkLine::Removed(line[1..].to_owned()));
        } else if line.starts_with(' ') {
            push_line(&mut current_hunk, HunkLine::Context(line[1..].to_owned()));
        } else if line.is_empty() {
            push_line(&mut current_hunk, HunkLine::Context(String::new()));
        }
    }
    if let Some(mut f) = current.take() {
        if let Some(h) = current_hunk.take() {
            f.hunks.push(h);
        }
        files.push(f);
    }
    Ok(files)
}

fn push_line(hunk: &mut Option<Hunk>, line: HunkLine) {
    if let Some(h) = hunk.as_mut() {
        h.lines.push(line);
    }
}

fn parse_diff_path(s: &str) -> Option<PathBuf> {
    let trimmed = s.trim();
    if trimmed == "/dev/null" {
        return None;
    }
    let stripped = trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed);
    let no_ts = stripped.split('\t').next().unwrap_or(stripped);
    Some(PathBuf::from(no_ts))
}

fn parse_hunk_header(rest: &str, lineno: usize) -> Result<Hunk, PatchError> {
    let mut parts = rest.split(' ');
    let old = parts.next().ok_or_else(|| PatchError::Malformed {
        line: lineno,
        detail: format!("expected old range in hunk header at line {}", lineno),
    })?;
    let new = parts.next().ok_or_else(|| PatchError::Malformed {
        line: lineno,
        detail: format!("expected new range in hunk header at line {}", lineno),
    })?;
    let (old_start, old_count) = parse_range(old.trim_start_matches('-'), lineno)?;
    let (new_start, new_count) = parse_range(new.trim_start_matches('+'), lineno)?;
    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(s: &str, lineno: usize) -> Result<(usize, usize), PatchError> {
    let (start, count) = match s.split_once(',') {
        Some((a, b)) => (a, b),
        None => (s, "1"),
    };
    let start = start.parse::<usize>().map_err(|_| PatchError::Malformed {
        line: lineno,
        detail: format!("malformed start in {:?}", s),
    })?;
    let count = count.parse::<usize>().map_err(|_| PatchError::Malformed {
        line: lineno,
        detail: format!("malformed count in {:?}", s),
    })?;
    Ok((start, count))
}
