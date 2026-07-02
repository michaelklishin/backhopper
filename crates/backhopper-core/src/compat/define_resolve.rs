// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves `?MACRO` and `#record` references a patch adds against the
//! target tree: the case where a hunk applies cleanly but the symbol it
//! references is defined nowhere the target reaches. Both are
//! directive-defined (`-define`, `-record`) and live in the file or its
//! includes, so one tree walk gathers both. `read_target` is injected:
//! core stays I/O-free.

use std::collections::BTreeSet;
use std::str::FromStr;

use crate::compat::added_file::is_stdlib_include_lib;
use crate::compat::added_lines::{AddedLinesSubject, file_line};
use crate::compat::qualified_call_resolve::TreeReader;
use crate::compat::source_attributes::{
    MacroDef, extract_defined_macro_values, extract_defined_macros, extract_defined_records,
    extract_includes, extract_macro_uses, extract_record_uses, is_predefined_macro,
    resolve_include,
};
use crate::compat::target_tree_index::TargetTreeIndex;
use crate::model::names::{RecordName, RelativePath};
use crate::model::verdict::{MacroValueTally, Reason};

/// `-include` follow depth bound: real header chains are a handful deep.
const MAX_INCLUDE_DEPTH: usize = 16;

/// Flag each added macro or record use that resolves to no definition on
/// the target and is neither predefined nor defined by the patch. One
/// reason per `(file, symbol)`.
pub fn analyse_define_symbols(
    subjects: &[AddedLinesSubject<'_>],
    target: &TargetTreeIndex,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> Vec<Reason> {
    let mut reasons = Vec::new();
    for subject in subjects {
        let macro_uses = extract_macro_uses(subject.added_text);
        let record_uses = extract_record_uses(subject.added_text);
        if macro_uses.is_empty() && record_uses.is_empty() {
            continue;
        }
        let defs = collect_target_defines(subject.source_path, target, read_target);
        // Incomplete define set: the symbol could live in a header we
        // could not read, so suppress rather than risk a false positive.
        if !defs.complete {
            continue;
        }
        let patch_macros = extract_defined_macros(subject.added_text);
        let mut flagged = BTreeSet::new();
        for u in macro_uses {
            if is_predefined_macro(&u.name)
                || patch_macros.contains(&u.name)
                || defs.macros.contains(&u.name)
                || !flagged.insert(u.name.clone())
            {
                continue;
            }
            reasons.push(Reason::MacroUndefinedOnTarget {
                source_path: subject.source_path.clone(),
                macro_name: u.name,
                line: file_line(subject.line_map, u.line),
            });
        }
        let patch_records = extract_defined_records(subject.added_text);
        let mut flagged = BTreeSet::new();
        for u in record_uses {
            if patch_records.contains(&u.name)
                || defs.records.contains(&u.name)
                || !flagged.insert(u.name.clone())
            {
                continue;
            }
            let Ok(record_name) = RecordName::from_str(&u.name) else {
                continue;
            };
            reasons.push(Reason::RecordUndefinedOnTarget {
                source_path: subject.source_path.clone(),
                record_name,
                line: file_line(subject.line_map, u.line),
            });
        }
    }
    reasons
}

/// Macros and records defined in the target version of `source_path` or
/// a header it transitively includes, plus whether the set is complete.
/// Incomplete when a non-stdlib include of a file we read could not be
/// resolved or read: a definition might hide in the header we missed.
struct TargetDefines {
    macros: BTreeSet<String>,
    records: BTreeSet<String>,
    complete: bool,
}

fn collect_target_defines(
    source_path: &RelativePath,
    target: &TargetTreeIndex,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> TargetDefines {
    let mut out = TargetDefines {
        macros: BTreeSet::new(),
        records: BTreeSet::new(),
        complete: true,
    };
    let mut visited: BTreeSet<RelativePath> = BTreeSet::new();
    let mut stack = vec![(source_path.clone(), 0usize)];
    while let Some((path, depth)) = stack.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        if depth > MAX_INCLUDE_DEPTH {
            out.complete = false;
            continue;
        }
        let Some(content) = read_target(&path) else {
            // An absent top file is the new-file case (complete); an
            // unreadable header leaves the set incomplete.
            if depth > 0 {
                out.complete = false;
            }
            continue;
        };
        out.macros.extend(extract_defined_macros(&content));
        out.records.extend(extract_defined_records(&content));
        for inc in extract_includes(&content) {
            if is_stdlib_include_lib(&inc.directive) {
                // Unreadable header: cannot prove a symbol undefined
                out.complete = false;
                continue;
            }
            match resolve_include(target, &path, &inc.directive) {
                Ok(resolved) => stack.push((resolved, depth + 1)),
                Err(_) => out.complete = false,
            }
        }
    }
    out
}

/// What the macro-value check produced: the drift reasons plus the
/// tally of what it did with each used name.
#[derive(Debug, Default)]
pub struct MacroValueAnalysis {
    pub reasons: Vec<Reason>,
    pub checks: MacroValueTally,
}

/// Compare the `-define` value of each macro the patch uses between
/// the two trees, when the definition lives in the touched file itself
/// on both sides, exactly once each. Everything else withholds and is
/// counted: a definition reached through an include (or present
/// same-file on one side only), an `-ifdef`-duplicated name, an
/// unreadable source side. A macro the patch itself redefines is
/// skipped: the patch carries the new value with it. One outcome per
/// `(file, name)`.
pub fn analyse_macro_values(
    subjects: &[AddedLinesSubject<'_>],
    read_target: TreeReader<'_>,
    read_source: Option<TreeReader<'_>>,
) -> MacroValueAnalysis {
    let mut analysis = MacroValueAnalysis::default();
    for subject in subjects {
        let uses = extract_macro_uses(subject.added_text);
        if uses.is_empty() {
            continue;
        }
        // An absent top file is the new-file case: nothing pre-existing
        // to compare, and the existence axis owns the rest.
        let Some(target_text) = read_target(subject.source_path) else {
            continue;
        };
        let patch_macros = extract_defined_macros(subject.added_text);
        let target_values = extract_defined_macro_values(&target_text);
        let mut source_values: Option<Option<_>> = None;
        let mut seen = BTreeSet::new();
        for u in uses {
            if is_predefined_macro(&u.name)
                || patch_macros.contains(&u.name)
                || !seen.insert(u.name.clone())
            {
                continue;
            }
            let Some(read_source) = read_source else {
                analysis.checks.withheld_no_source += 1;
                continue;
            };
            let source_values = source_values.get_or_insert_with(|| {
                read_source(subject.source_path).map(|t| extract_defined_macro_values(&t))
            });
            let Some(source_values) = source_values.as_ref() else {
                analysis.checks.withheld_no_source += 1;
                continue;
            };
            match (target_values.get(&u.name), source_values.get(&u.name)) {
                (Some(target_defs), Some(source_defs)) => {
                    let (Some(target_def), Some(source_def)) =
                        (single(target_defs), single(source_defs))
                    else {
                        analysis.checks.withheld_multiple_defines += 1;
                        continue;
                    };
                    analysis.checks.compared += 1;
                    if target_def != source_def {
                        analysis.reasons.push(Reason::MacroValueDrift {
                            source_path: subject.source_path.clone(),
                            macro_name: u.name,
                            source_value: rendered(source_def),
                            target_value: rendered(target_def),
                            line: file_line(subject.line_map, u.line),
                        });
                    }
                }
                _ => analysis.checks.withheld_definition_elsewhere += 1,
            }
        }
    }
    analysis
}

fn single(defs: &[MacroDef]) -> Option<&MacroDef> {
    match defs {
        [one] => Some(one),
        _ => None,
    }
}

/// A function-like define renders with its parameter count, so a
/// form-only difference still shows two distinguishable values.
fn rendered(def: &MacroDef) -> String {
    match def.params {
        Some(n) => format!("({n} args) {}", def.body),
        None => def.body.clone(),
    }
}
