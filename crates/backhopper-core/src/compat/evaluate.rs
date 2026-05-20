// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-pin evaluation of an analyzed patch. Pure functions: take a
//! snapshot plus optional file bytes and scope, return reasons.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::str;

use crate::compat::arg_shape::{ArgShape, satisfies_any};
use crate::compat::patch::{EvaluationFiles, Hunk, HunkLine, PatchedFile};
use crate::compat::scope::PinScope;
use crate::model::names::{Arity, FieldName, FunctionName, Mfa, ModuleName, RecordName};
use crate::model::snapshot::{ArityMatch, FunArity, Module, Snapshot, Visibility, state};
use crate::model::symbol::{SymbolKind, SymbolRef};
use crate::model::verdict::{Reason, SourceDelta, Verdict};

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
) -> EvaluationResult {
    let mut reasons: Vec<Reason> = Vec::new();
    if let Some(pf) = pin_files {
        check_files_against_pin(files, pf, &mut reasons);
    }
    let defined_index: HashSet<&SymbolRef> = defined.iter().collect();
    let mut tracked_refs: Vec<SymbolRef> = Vec::new();
    let mut source_deltas: Vec<SourceDelta> = Vec::new();
    for r in referenced {
        if defined_index.contains(r) {
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
                    continue;
                }
                check_record_against_source(name, snapshot, source_snapshot, &mut reasons);
            }
            _ => {}
        }
    }
    check_clause_mismatches(call_args, snapshot, scope, &mut reasons);
    for path in unsupported_files {
        reasons.push(Reason::UnsupportedFileType { path: path.clone() });
    }
    EvaluationResult {
        verdict: Verdict::from_reasons(reasons),
        tracked_refs,
        source_deltas,
    }
}

pub(crate) struct EvaluationResult {
    pub verdict: Verdict,
    pub tracked_refs: Vec<SymbolRef>,
    pub source_deltas: Vec<SourceDelta>,
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

fn check_files_against_pin(
    files: &[PatchedFile],
    pin_files: &EvaluationFiles,
    reasons: &mut Vec<Reason>,
) {
    for file in files {
        let path = match (&file.new_path, &file.old_path) {
            (Some(p), _) | (None, Some(p)) => p,
            (None, None) => continue,
        };
        let Some(slot) = pin_files.get(path) else {
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
    source_snapshot: Option<&Snapshot<state::Canonical>>,
    reasons: &mut Vec<Reason>,
) {
    let Some(module) = snapshot.module_named(&mfa.module) else {
        reasons.push(Reason::MissingSymbol {
            symbol: r.clone(),
            first_seen_at_tag: None,
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

fn collect_alt_arities_in(module: &Module, function: &FunctionName) -> Vec<Arity> {
    module
        .exports
        .iter()
        .filter(|fa| &fa.name == function)
        .map(|fa| fa.arity)
        .collect()
}
