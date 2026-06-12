// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-pin evaluation of an analyzed patch. Pure functions: take a
//! snapshot plus optional file bytes and scope, return reasons.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::str::{self, FromStr};

use crate::compat::arg_shape::{ArgShape, satisfies_any};
use crate::compat::patch::{EvaluationFiles, Hunk, HunkLine, PatchedFile};
use crate::compat::scope::PinScope;
use crate::config::FamilyDefaults;
use crate::model::names::{
    Arity, FieldName, FunctionName, MacroName, Mfa, ModuleName, RecordName, TypeName,
};
use crate::model::snapshot::{ArityMatch, FunArity, Module, Snapshot, Visibility, state};
use crate::model::spec_ast::SpecType;
use crate::model::spec_parser::parse_signature_return;
use crate::model::symbol::{RefOrigin, SymbolKind, SymbolRef};
use crate::model::verdict::{
    ArtifactKind, ConflictMarker, HunkTally, Reason, SnapshotSide, SourceDelta, Verdict,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_pin(
    files: &[PatchedFile],
    referenced: &[SymbolRef],
    defined: &[SymbolRef],
    unsupported_files: &[PathBuf],
    call_args: &[(Mfa, Vec<ArgShape>)],
    snapshot: &Snapshot<state::Canonical>,
    source_snapshot: Option<&Snapshot<state::Canonical>>,
    pin_files: Option<&EvaluationFiles>,
    scope: Option<&PinScope>,
    family_defaults: Option<&FamilyDefaults>,
) -> EvaluationResult {
    let mut reasons: Vec<Reason> = Vec::new();
    check_syntactic_artifacts(files, &mut reasons);
    let mut postimage = None;
    if let Some(pf) = pin_files {
        let tally = check_files_against_pin(files, pf, snapshot, &mut reasons);
        if tally.considered > 0 {
            postimage = Some(tally);
        }
    }
    if let Some(fd) = family_defaults {
        check_versioned_machine_data(files, snapshot, source_snapshot, fd, &mut reasons);
    }
    let defined_index: HashSet<&SymbolKind> = defined.iter().map(|d| &d.kind).collect();
    let mut tracked_refs: Vec<SymbolRef> = Vec::new();
    let mut context_missing: BTreeMap<ModuleName, usize> = BTreeMap::new();
    let mut source_deltas: Vec<SourceDelta> = Vec::new();
    for r in referenced {
        if defined_index.contains(&r.kind) {
            continue;
        }
        // Context-line references are pre-existing target facts: an
        // unresolved one is surfaced as a diagnostic, never a reason.
        if r.origin == RefOrigin::Context {
            tally_context_miss(r, snapshot, scope, &mut context_missing);
            continue;
        }
        match &r.kind {
            SymbolKind::Function { mfa } => {
                if let Some(s) = scope
                    && !s.contains_module(&mfa.module)
                {
                    continue;
                }
                tracked_refs.push(r.clone());
                analyze_function_reference(r, mfa, snapshot, source_snapshot, &mut reasons);
                if let Some(src) = source_snapshot {
                    record_source_delta(mfa, snapshot, src, &mut source_deltas);
                }
            }
            SymbolKind::FunctionAnyArity { module, function } => {
                if let Some(s) = scope
                    && !s.contains_module(module)
                {
                    continue;
                }
                tracked_refs.push(r.clone());
                analyze_any_arity_reference(r, module, function, snapshot, &mut reasons);
            }
            SymbolKind::Record { name } => {
                if let Some(s) = scope
                    && !s.contains_record(name)
                {
                    continue;
                }
                tracked_refs.push(r.clone());
                if !record_present(snapshot, name) {
                    reasons.push(Reason::MissingSymbol {
                        symbol: r.clone(),
                        first_seen_at_tag: None,
                        needs_pin_at_least: None,
                        suggested_replacement: None,
                    });
                    continue;
                }
                check_record_against_source(name, snapshot, source_snapshot, &mut reasons);
            }
            SymbolKind::Type {
                module,
                name,
                arity,
            } => {
                if let Some(s) = scope
                    && !s.contains_module(module)
                {
                    continue;
                }
                tracked_refs.push(r.clone());
                if !type_exported(snapshot, module, name, *arity) {
                    reasons.push(Reason::MissingType {
                        module: module.clone(),
                        name: name.clone(),
                        arity: *arity,
                    });
                }
            }
            _ => {}
        }
    }
    check_clause_mismatches(call_args, snapshot, scope, &mut reasons);
    if let Some(src) = source_snapshot {
        check_behaviour_conformance(snapshot, src, scope, &mut reasons);
        check_return_shapes(referenced, snapshot, src, scope, &mut reasons);
    }
    for path in unsupported_files {
        reasons.push(Reason::UnsupportedFileType { path: path.clone() });
    }
    EvaluationResult {
        verdict: Verdict::from_reasons(reasons),
        tracked_refs,
        context_missing,
        source_deltas,
        postimage,
    }
}

pub(crate) struct EvaluationResult {
    pub verdict: Verdict,
    pub tracked_refs: Vec<SymbolRef>,
    /// In-scope context-line references that do not resolve at this
    /// pin, tallied per module. Diagnostic only.
    pub context_missing: BTreeMap<ModuleName, usize>,
    pub source_deltas: Vec<SourceDelta>,
    /// Content-presence tally over the pin's files, when any hunk
    /// added lines and the pin had file bytes to compare against.
    pub postimage: Option<PostimageTally>,
}

/// One unresolved in-scope context reference: tallied under its
/// module so the operator can see what the surrounding target code
/// relies on without it gating the verdict.
fn tally_context_miss(
    r: &SymbolRef,
    snapshot: &Snapshot<state::Canonical>,
    scope: Option<&PinScope>,
    out: &mut BTreeMap<ModuleName, usize>,
) {
    let module = match &r.kind {
        SymbolKind::Function { mfa } => &mfa.module,
        SymbolKind::FunctionAnyArity { module, .. } | SymbolKind::Type { module, .. } => module,
        _ => return,
    };
    if let Some(s) = scope
        && !s.contains_module(module)
    {
        return;
    }
    let resolved = match &r.kind {
        SymbolKind::Function { mfa } => {
            function_exported(snapshot, &mfa.module, &mfa.function, mfa.arity)
        }
        SymbolKind::FunctionAnyArity { module, function } => {
            function_exported_any_arity(snapshot, module, function)
        }
        SymbolKind::Type {
            module,
            name,
            arity,
        } => type_exported(snapshot, module, name, *arity),
        _ => unreachable!("filtered above"),
    };
    if !resolved {
        *out.entry(module.clone()).or_insert(0) += 1;
    }
}

fn function_exported(
    snapshot: &Snapshot<state::Canonical>,
    module: &ModuleName,
    function: &FunctionName,
    arity: Arity,
) -> bool {
    let Some(m) = snapshot.module_named(module) else {
        return false;
    };
    let target = FunArity {
        name: function.clone(),
        arity,
    };
    m.exports.binary_search(&target).is_ok()
}

fn function_exported_any_arity(
    snapshot: &Snapshot<state::Canonical>,
    module: &ModuleName,
    function: &FunctionName,
) -> bool {
    snapshot
        .module_named(module)
        .is_some_and(|m| m.exports.iter().any(|fa| &fa.name == function))
}

/// Lookup for a reference whose arity the extractor could not pin
/// down: any export with the name resolves it; an arity mismatch can
/// never fire from a guess.
fn analyze_any_arity_reference(
    r: &SymbolRef,
    module: &ModuleName,
    function: &FunctionName,
    snapshot: &Snapshot<state::Canonical>,
    reasons: &mut Vec<Reason>,
) {
    let Some(m) = snapshot.module_named(module) else {
        reasons.push(Reason::MissingSymbol {
            symbol: r.clone(),
            first_seen_at_tag: None,
            needs_pin_at_least: None,
            suggested_replacement: None,
        });
        return;
    };
    if m.visibility == Visibility::Hidden {
        reasons.push(Reason::NowHidden {
            module: module.clone(),
        });
        return;
    }
    if m.exports.iter().any(|fa| &fa.name == function) {
        return;
    }
    reasons.push(Reason::MissingSymbol {
        symbol: r.clone(),
        first_seen_at_tag: None,
        needs_pin_at_least: None,
        suggested_replacement: None,
    });
}

/// Per-pin counters for the tier-2 content-presence check; the
/// per-pin precursor of the envelope's `ContentPresence`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct PostimageTally {
    pub applied: usize,
    pub considered: usize,
    pub ambiguous: usize,
    pub low_confidence: usize,
    pub per_file: BTreeMap<PathBuf, HunkTally>,
}

fn record_source_delta(
    mfa: &Mfa,
    target: &Snapshot<state::Canonical>,
    source: &Snapshot<state::Canonical>,
    out: &mut Vec<SourceDelta>,
) {
    let already = out
        .iter()
        .any(|d| d.module == mfa.module && d.function == mfa.function && d.arity == mfa.arity);
    if already {
        return;
    }
    let Some(target_spec) = find_spec(target, &mfa.module, &mfa.function, mfa.arity) else {
        return;
    };
    let Some(source_spec) = find_spec(source, &mfa.module, &mfa.function, mfa.arity) else {
        return;
    };
    if target_spec == source_spec {
        return;
    }
    out.push(SourceDelta {
        module: mfa.module.clone(),
        function: mfa.function.clone(),
        arity: mfa.arity,
        source_spec: source_spec.to_string(),
        target_spec: target_spec.to_string(),
    });
}

fn check_clause_mismatches(
    call_args: &[(Mfa, Vec<ArgShape>)],
    snapshot: &Snapshot<state::Canonical>,
    scope: Option<&PinScope>,
    reasons: &mut Vec<Reason>,
) {
    let signature_already: HashSet<(ModuleName, FunctionName, Arity)> = reasons
        .iter()
        .filter_map(|r| match r {
            Reason::SignatureChanged {
                module,
                function,
                arity,
                ..
            } => Some((module.clone(), function.clone(), *arity)),
            _ => None,
        })
        .collect();
    let mut grouped: BTreeMap<(ModuleName, FunctionName, Arity), Vec<&Vec<ArgShape>>> =
        BTreeMap::new();
    for (mfa, args) in call_args {
        if let Some(s) = scope
            && !s.contains_module(&mfa.module)
        {
            continue;
        }
        grouped
            .entry((mfa.module.clone(), mfa.function.clone(), mfa.arity))
            .or_default()
            .push(args);
    }
    for ((module_name, function_name, arity), call_groups) in grouped {
        if signature_already.contains(&(module_name.clone(), function_name.clone(), arity)) {
            continue;
        }
        let Some(module) = snapshot.module_named(&module_name) else {
            continue;
        };
        let fun_arity = FunArity {
            name: function_name.clone(),
            arity,
        };
        let Some(clauses) = module.clause_heads.get(&fun_arity) else {
            continue;
        };
        let Some(failing) = call_groups
            .into_iter()
            .find(|args| !satisfies_any(args, clauses))
        else {
            continue;
        };
        reasons.push(Reason::ClauseMismatch {
            module: module_name,
            function: function_name,
            arity,
            call_args: failing.clone(),
            pin_clauses: clauses.clone(),
        });
    }
}

/// Map a touched-file path to the module name implied by its basename.
/// Returns `None` for paths whose basename does not match Erlang module
/// naming conventions (e.g. non-`.erl`/`.hrl` files, or names with chars
/// `ModuleName::new` rejects).
fn file_to_module_name(path: &Path) -> Option<ModuleName> {
    let basename = path.file_name()?.to_str()?;
    let stem = basename.strip_suffix(".erl")?;
    ModuleName::from_str(stem).ok()
}

fn touched_module_set(files: &[PatchedFile]) -> BTreeSet<ModuleName> {
    let mut set = BTreeSet::new();
    for f in files {
        let path = f.new_path.as_ref().or(f.old_path.as_ref());
        let Some(path) = path else { continue };
        let ext = path.extension().and_then(|s| s.to_str());
        if !matches!(ext, Some("erl") | Some("hrl")) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(m) = ModuleName::new(stem) {
            set.insert(m);
        }
    }
    set
}

fn recorded_wire_macros<'a>(
    snapshot: &'a Snapshot<state::Canonical>,
    module: &ModuleName,
) -> BTreeSet<&'a str> {
    snapshot
        .module_named(module)
        .map(|m| {
            m.wire_constants
                .iter()
                .map(|wc| wc.macro_name.as_str())
                .collect()
        })
        .unwrap_or_default()
}

fn check_versioned_machine_data(
    files: &[PatchedFile],
    target: &Snapshot<state::Canonical>,
    source: Option<&Snapshot<state::Canonical>>,
    defaults: &FamilyDefaults,
    reasons: &mut Vec<Reason>,
) {
    let touched = touched_module_set(files);
    for impl_decl in &defaults.versioned_machine_impls {
        let Ok(module) = ModuleName::new(&impl_decl.module) else {
            continue;
        };
        if !touched.contains(&module) {
            continue;
        }
        let on_target = target
            .module_named(&module)
            .and_then(|m| m.versioned_machine_version.as_ref())
            .is_some();
        let on_source = source
            .and_then(|s| s.module_named(&module))
            .and_then(|m| m.versioned_machine_version.as_ref())
            .is_some();
        let side = match (source.is_some(), on_source, on_target) {
            (true, false, false) => Some(SnapshotSide::Both),
            (true, false, true) => Some(SnapshotSide::Source),
            (true, true, false) => Some(SnapshotSide::Target),
            (true, true, true) => None,
            (false, _, false) => Some(SnapshotSide::Target),
            (false, _, true) => None,
        };
        if let Some(side) = side {
            reasons.push(Reason::VersionedMachineSnapshotMissing { module, side });
        }
    }
    for wc_decl in &defaults.wire_constants {
        let Ok(module) = ModuleName::new(&wc_decl.module) else {
            continue;
        };
        if !touched.contains(&module) {
            continue;
        }
        let target_present = recorded_wire_macros(target, &module);
        let missing_target: Vec<MacroName> = wc_decl
            .macros
            .iter()
            .filter(|m| !target_present.contains(m.as_str()))
            .filter_map(|m| MacroName::new(m).ok())
            .collect();
        let Some(src) = source else {
            if !missing_target.is_empty() {
                let mut macros = missing_target;
                macros.sort();
                reasons.push(Reason::WireConstantBindingsMissing {
                    module,
                    macros,
                    side: SnapshotSide::Target,
                });
            }
            continue;
        };
        let source_present = recorded_wire_macros(src, &module);
        let missing_source: Vec<MacroName> = wc_decl
            .macros
            .iter()
            .filter(|m| !source_present.contains(m.as_str()))
            .filter_map(|m| MacroName::new(m).ok())
            .collect();
        match (missing_source.is_empty(), missing_target.is_empty()) {
            (true, true) => {}
            (false, true) => {
                let mut macros = missing_source;
                macros.sort();
                reasons.push(Reason::WireConstantBindingsMissing {
                    module,
                    macros,
                    side: SnapshotSide::Source,
                });
            }
            (true, false) => {
                let mut macros = missing_target;
                macros.sort();
                reasons.push(Reason::WireConstantBindingsMissing {
                    module,
                    macros,
                    side: SnapshotSide::Target,
                });
            }
            (false, false) if missing_source == missing_target => {
                let mut macros = missing_target;
                macros.sort();
                reasons.push(Reason::WireConstantBindingsMissing {
                    module,
                    macros,
                    side: SnapshotSide::Both,
                });
            }
            (false, false) => {
                let mut src_macros = missing_source;
                src_macros.sort();
                let mut tgt_macros = missing_target;
                tgt_macros.sort();
                reasons.push(Reason::WireConstantBindingsMissing {
                    module: module.clone(),
                    macros: src_macros,
                    side: SnapshotSide::Source,
                });
                reasons.push(Reason::WireConstantBindingsMissing {
                    module,
                    macros: tgt_macros,
                    side: SnapshotSide::Target,
                });
            }
        }
    }
}

fn check_syntactic_artifacts(files: &[PatchedFile], reasons: &mut Vec<Reason>) {
    for file in files {
        let path = match (&file.new_path, &file.old_path) {
            (Some(p), _) | (None, Some(p)) => p,
            (None, None) => continue,
        };
        for hunk in &file.hunks {
            let mut new_line = hunk.new_start;
            for line in &hunk.lines {
                match line {
                    HunkLine::Context(_) => {
                        new_line += 1;
                    }
                    HunkLine::Added(text) => {
                        if let Some(marker) = ConflictMarker::detect(text) {
                            reasons.push(Reason::SyntacticArtifact {
                                path: path.clone(),
                                line: new_line,
                                artifact: ArtifactKind::ConflictMarker { marker },
                            });
                        }
                        new_line += 1;
                    }
                    HunkLine::Removed(_) => {}
                }
            }
        }
    }
}

fn check_files_against_pin(
    files: &[PatchedFile],
    pin_files: &EvaluationFiles,
    snapshot: &Snapshot<state::Canonical>,
    reasons: &mut Vec<Reason>,
) -> PostimageTally {
    let mut tally = PostimageTally::default();
    for file in files {
        let path = match (&file.new_path, &file.old_path) {
            (Some(p), _) | (None, Some(p)) => p,
            (None, None) => continue,
        };
        let Some(slot) = pin_files.get(path) else {
            continue;
        };
        let Some(bytes) = slot else {
            if let Some(module) = file_to_module_name(path)
                && snapshot.module_named(&module).is_some()
            {
                reasons.push(Reason::ModuleRelocated {
                    module,
                    patch_path: path.clone(),
                });
            } else {
                reasons.push(Reason::FileAbsent { path: path.clone() });
            }
            continue;
        };
        let target_lines = match str::from_utf8(bytes) {
            Ok(s) => s.lines().collect::<Vec<&str>>(),
            Err(_) => continue,
        };
        for (idx, hunk) in file.hunks.iter().enumerate() {
            let pre = classify_preimage(hunk, &target_lines);
            tally_postimage(hunk, &pre, &target_lines, path, &mut tally);
            match pre {
                PreimageMatch::Exact => {}
                PreimageMatch::Drifted { line_delta } => {
                    reasons.push(Reason::PreimageDrifted {
                        path: path.clone(),
                        hunk_index: idx,
                        line_delta,
                    });
                }
                PreimageMatch::Missing { excerpt } => {
                    reasons.push(Reason::PreimageMissing {
                        path: path.clone(),
                        hunk_index: idx,
                        preimage_excerpt: excerpt,
                    });
                }
            }
        }
    }
    tally
}

/// The dual of `classify_preimage`: does the hunk's post-image
/// (Context plus Added lines) already exist in the pin's file? Only
/// hunks that add lines are considered; deletions get no signal.
fn tally_postimage(
    hunk: &Hunk,
    pre: &PreimageMatch,
    target_lines: &[&str],
    path: &Path,
    tally: &mut PostimageTally,
) {
    let added = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Added(_)))
        .count();
    if added == 0 {
        return;
    }
    let postimage: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|l| match l {
            HunkLine::Context(t) | HunkLine::Added(t) => Some(t.as_str()),
            HunkLine::Removed(_) => None,
        })
        .collect();
    let context = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_)))
        .count();
    let preimage_empty = hunk.lines.iter().all(|l| matches!(l, HunkLine::Added(_)));
    tally.considered += 1;
    let entry = tally.per_file.entry(path.to_path_buf()).or_default();
    entry.considered += 1;
    if find_subsequence(&postimage, target_lines).is_none() {
        return;
    }
    // An empty preimage classifies `Exact`, so added files must be
    // decided by the postimage alone.
    let pre_matches = !preimage_empty && !matches!(pre, PreimageMatch::Missing { .. });
    if pre_matches {
        tally.ambiguous += 1;
        entry.ambiguous += 1;
        return;
    }
    // Context lines make the postimage block a distinctive needle; a
    // context-less near-empty block is too weak to call applied.
    if context == 0 && postimage.len() < 2 {
        tally.low_confidence += 1;
        entry.low_confidence += 1;
    } else {
        tally.applied += 1;
        entry.applied += 1;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PreimageMatch {
    Exact,
    Drifted { line_delta: isize },
    Missing { excerpt: String },
}

/// Classify how the hunk's preimage block (Context and Removed lines)
/// matches against `target_lines`. `Exact` is the happy path (preimage
/// at `hunk.old_start - 1`); `Drifted` recovers an offset; `Missing`
/// returns up to the first three preimage lines as an excerpt.
fn classify_preimage(hunk: &Hunk, target_lines: &[&str]) -> PreimageMatch {
    let preimage: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|l| match l {
            HunkLine::Context(t) | HunkLine::Removed(t) => Some(t.as_str()),
            HunkLine::Added(_) => None,
        })
        .collect();
    if preimage.is_empty() {
        return PreimageMatch::Exact;
    }
    let expected = hunk.old_start.saturating_sub(1);
    if matches_at(&preimage, target_lines, expected) {
        return PreimageMatch::Exact;
    }
    if let Some(found) = find_subsequence(&preimage, target_lines) {
        let delta = found as isize - expected as isize;
        return PreimageMatch::Drifted { line_delta: delta };
    }
    let excerpt = preimage
        .iter()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    PreimageMatch::Missing { excerpt }
}

fn matches_at(preimage: &[&str], target: &[&str], start: usize) -> bool {
    if start + preimage.len() > target.len() {
        return false;
    }
    preimage
        .iter()
        .enumerate()
        .all(|(i, line)| target[start + i] == *line)
}

fn find_subsequence(preimage: &[&str], target: &[&str]) -> Option<usize> {
    if preimage.is_empty() || preimage.len() > target.len() {
        return None;
    }
    let last = target.len() - preimage.len();
    (0..=last).find(|&i| matches_at(preimage, target, i))
}

fn analyze_function_reference(
    r: &SymbolRef,
    mfa: &Mfa,
    snapshot: &Snapshot<state::Canonical>,
    source_snapshot: Option<&Snapshot<state::Canonical>>,
    reasons: &mut Vec<Reason>,
) {
    let Some(module) = snapshot.module_named(&mfa.module) else {
        reasons.push(Reason::MissingSymbol {
            symbol: r.clone(),
            first_seen_at_tag: None,
            needs_pin_at_least: None,
            suggested_replacement: None,
        });
        return;
    };
    if module.visibility == Visibility::Hidden {
        reasons.push(Reason::NowHidden {
            module: mfa.module.clone(),
        });
        return;
    }
    if is_function_deprecated_in(module, &mfa.function, mfa.arity) {
        reasons.push(Reason::DeprecatedUsage {
            symbol: r.clone(),
            since: None,
            replacement: None,
        });
    }
    let target = FunArity {
        name: mfa.function.clone(),
        arity: mfa.arity,
    };
    if module.exports.binary_search(&target).is_err() {
        let alt_arities = collect_alt_arities_in(module, &mfa.function);
        if alt_arities.is_empty() {
            reasons.push(Reason::MissingSymbol {
                symbol: r.clone(),
                first_seen_at_tag: None,
                needs_pin_at_least: None,
                suggested_replacement: None,
            });
        } else {
            reasons.push(Reason::ArityChanged {
                module: mfa.module.clone(),
                function: mfa.function.clone(),
                expected: mfa.arity,
                found: alt_arities,
                expected_available_at: None,
                needs_pin_at_least: None,
            });
        }
        return;
    }
    if let Some(src) = source_snapshot {
        let target_spec = find_spec(snapshot, &mfa.module, &mfa.function, mfa.arity);
        let source_spec = find_spec(src, &mfa.module, &mfa.function, mfa.arity);
        if let (Some(t), Some(s)) = (target_spec, source_spec)
            && t != s
        {
            reasons.push(Reason::SignatureChanged {
                module: mfa.module.clone(),
                function: mfa.function.clone(),
                arity: mfa.arity,
                expected_spec: s.to_string(),
                found_spec: t.to_string(),
            });
        }
    }
}

fn check_behaviour_conformance(
    snapshot: &Snapshot<state::Canonical>,
    source_snapshot: &Snapshot<state::Canonical>,
    scope: Option<&PinScope>,
    reasons: &mut Vec<Reason>,
) {
    for source_module in source_snapshot.modules() {
        if source_module.callbacks.is_empty() && source_module.optional_callbacks.is_empty() {
            continue;
        }
        let Some(pin_module) = snapshot.module_named(&source_module.name) else {
            continue;
        };
        let implementers = snapshot.implementers_of(&source_module.name);
        if implementers.is_empty() {
            continue;
        }
        let optional_at_source: HashSet<FunArity> =
            source_module.optional_callbacks.iter().cloned().collect();
        let optional_at_pin: HashSet<FunArity> =
            pin_module.optional_callbacks.iter().cloned().collect();
        let source_by_arity: BTreeMap<FunArity, &str> = source_module
            .callbacks
            .iter()
            .map(|c| {
                (
                    FunArity {
                        name: c.name.clone(),
                        arity: c.arity,
                    },
                    c.signature.as_str(),
                )
            })
            .collect();
        let pin_by_arity: BTreeMap<FunArity, &str> = pin_module
            .callbacks
            .iter()
            .map(|c| {
                (
                    FunArity {
                        name: c.name.clone(),
                        arity: c.arity,
                    },
                    c.signature.as_str(),
                )
            })
            .collect();
        for (key, source_sig) in &source_by_arity {
            if let Some(pin_sig) = pin_by_arity.get(key) {
                if signatures_differ(source_sig, pin_sig) {
                    for impl_module in &implementers {
                        if !implementer_in_scope(impl_module, scope) {
                            continue;
                        }
                        reasons.push(Reason::BehaviourCallbackSignatureChanged {
                            behaviour: source_module.name.clone(),
                            callback: key.name.clone(),
                            arity: key.arity,
                            expected_after_patch: (*source_sig).to_owned(),
                            implementer: impl_module.name.clone(),
                            implementer_signature: (*pin_sig).to_owned(),
                        });
                    }
                }
            } else if !optional_at_source.contains(key) {
                for impl_module in &implementers {
                    if !implementer_in_scope(impl_module, scope) {
                        continue;
                    }
                    if impl_module.exports.binary_search(key).is_err() {
                        reasons.push(Reason::BehaviourCallbackAdded {
                            behaviour: source_module.name.clone(),
                            callback: key.name.clone(),
                            arity: key.arity,
                            implementer: impl_module.name.clone(),
                        });
                    }
                }
            }
        }
        for key in pin_by_arity.keys() {
            if !source_by_arity.contains_key(key) && !optional_at_pin.contains(key) {
                for impl_module in &implementers {
                    if !implementer_in_scope(impl_module, scope) {
                        continue;
                    }
                    if impl_module.exports.binary_search(key).is_ok() {
                        reasons.push(Reason::BehaviourCallbackRemoved {
                            behaviour: source_module.name.clone(),
                            callback: key.name.clone(),
                            arity: key.arity,
                            implementer: impl_module.name.clone(),
                        });
                    }
                }
            }
        }
    }
}

/// Compare two signature strings, treating equivalent return-type ASTs
/// as equal even when whitespace, atom-quote variation, or union ordering
/// differs in the raw text.
fn signatures_differ(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let ast_a = parse_signature_return(a);
    let ast_b = parse_signature_return(b);
    ast_a != ast_b
}

fn implementer_in_scope(module: &Module, scope: Option<&PinScope>) -> bool {
    match scope {
        Some(s) => s.contains_module(&module.name),
        None => true,
    }
}

/// For each referenced MFA whose pin-side and source-side specs both
/// parse into structurally-precise return types, fire
/// `ReturnShapeMismatch` when the shapes disagree. `Unknown` on either
/// side suppresses the reason: this rule never fires speculatively.
fn check_return_shapes(
    referenced: &[SymbolRef],
    snapshot: &Snapshot<state::Canonical>,
    source: &Snapshot<state::Canonical>,
    scope: Option<&PinScope>,
    reasons: &mut Vec<Reason>,
) {
    let mut seen: HashSet<(ModuleName, FunctionName, Arity)> = HashSet::new();
    for r in referenced {
        // Context-line references never drive verdicts.
        if r.origin == RefOrigin::Context {
            continue;
        }
        let SymbolKind::Function { mfa } = &r.kind else {
            continue;
        };
        if let Some(s) = scope
            && !s.contains_module(&mfa.module)
        {
            continue;
        }
        let key = (mfa.module.clone(), mfa.function.clone(), mfa.arity);
        if !seen.insert(key) {
            continue;
        }
        let Some(pin_sig) = find_spec(snapshot, &mfa.module, &mfa.function, mfa.arity) else {
            continue;
        };
        let Some(src_sig) = find_spec(source, &mfa.module, &mfa.function, mfa.arity) else {
            continue;
        };
        if pin_sig == src_sig {
            continue;
        }
        let pin_ast = parse_signature_return(pin_sig);
        let src_ast = parse_signature_return(src_sig);
        if matches!(pin_ast, SpecType::Unknown) || matches!(src_ast, SpecType::Unknown) {
            continue;
        }
        if pin_ast.matches(&src_ast) {
            continue;
        }
        reasons.push(Reason::ReturnShapeMismatch {
            module: mfa.module.clone(),
            function: mfa.function.clone(),
            arity: mfa.arity,
            source_signature: src_sig.to_owned(),
            pin_signature: pin_sig.to_owned(),
        });
    }
}

fn check_record_against_source(
    name: &RecordName,
    snapshot: &Snapshot<state::Canonical>,
    source_snapshot: Option<&Snapshot<state::Canonical>>,
    reasons: &mut Vec<Reason>,
) {
    let Some(src) = source_snapshot else { return };
    let target_fields = find_record_fields(snapshot, name);
    let source_fields = find_record_fields(src, name);
    if let (Some(t), Some(s)) = (target_fields, source_fields)
        && t != s
    {
        reasons.push(Reason::RecordFieldsChanged {
            record: name.clone(),
            expected: s,
            found: t,
        });
    }
}

fn find_spec<'a>(
    snapshot: &'a Snapshot<state::Canonical>,
    module: &ModuleName,
    function: &FunctionName,
    arity: Arity,
) -> Option<&'a str> {
    let m = snapshot.module_named(module)?;
    let idx = m
        .specs
        .binary_search_by(|s| s.name.cmp(function).then(s.arity.cmp(&arity)))
        .ok()?;
    Some(m.specs[idx].signature.as_str())
}

fn find_record_fields(
    snapshot: &Snapshot<state::Canonical>,
    name: &RecordName,
) -> Option<Vec<FieldName>> {
    snapshot.headers().iter().find_map(|h| {
        h.records
            .iter()
            .find(|r| &r.name == name)
            .map(|r| r.fields.iter().map(|f| f.name.clone()).collect())
    })
}

fn is_function_deprecated_in(module: &Module, function: &FunctionName, arity: Arity) -> bool {
    for d in &module.deprecations {
        if d.module_wide {
            return true;
        }
        if let Some(f) = &d.function
            && f == function
        {
            match d.arity_match {
                ArityMatch::Any => return true,
                ArityMatch::Exact { arity: a } if a == arity => return true,
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

fn type_exported(
    snapshot: &Snapshot<state::Canonical>,
    module: &ModuleName,
    name: &TypeName,
    arity: Arity,
) -> bool {
    let Some(m) = snapshot.module_named(module) else {
        return false;
    };
    m.export_types
        .binary_search_by(|t| t.name.cmp(name).then(t.arity.cmp(&arity)))
        .is_ok()
}

fn collect_alt_arities_in(module: &Module, function: &FunctionName) -> Vec<Arity> {
    module
        .exports
        .iter()
        .filter(|fa| &fa.name == function)
        .map(|fa| fa.arity)
        .collect()
}
