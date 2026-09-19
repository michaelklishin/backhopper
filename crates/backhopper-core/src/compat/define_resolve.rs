// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves `?MACRO` and `#record` references a patch adds against the
//! target tree: the case where a hunk applies cleanly but the symbol it
//! references is defined nowhere the target reaches. Both are
//! directive-defined (`-define`, `-record`) and defined in the file or its
//! includes, so one tree walk gathers both. It gathers type
//! declarations too: the exported-type axis needs the same closure.
//! `read_target` is injected: core stays I/O-free.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use crate::compat::added_file::is_stdlib_include_lib;
use crate::compat::added_lines::{AddedLinesSubject, file_line};
use crate::compat::qualified_call_resolve::TreeReader;
use crate::compat::source_attributes::{
    MacroDef, extract_defined_macro_values, extract_defined_macros, extract_defined_records,
    extract_defined_types, extract_includes, extract_macro_uses, extract_record_uses,
    has_macro_expanded_attribute, is_predefined_macro, resolve_include,
};
use crate::compat::target_tree_index::TargetTreeIndex;
use crate::model::names::{Arity, MacroName, RecordName, RelativePath, TypeName};
use crate::model::verdict::{MacroValueTally, Reason};

/// `-include` follow depth bound: real header chains are a handful deep.
const MAX_INCLUDE_DEPTH: usize = 16;

/// Flag each added macro or record use that resolves to no definition on
/// the target and is neither predefined nor defined by the patch. One
/// reason per `(file, symbol)`.
pub fn analyse_define_symbols(
    subjects: &[AddedLinesSubject<'_>],
    patch_added: &BTreeMap<RelativePath, String>,
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
        let defs = collect_target_defines(subject, patch_added, target, read_target);
        // Incomplete define set: the symbol could be in an unreadable header, so suppress rather than risk a false positive.
        if defs.coverage.hides_macros_or_records() {
            continue;
        }
        let patch_macros = extract_defined_macros(subject.added_text);
        let mut flagged = BTreeSet::new();
        for u in macro_uses {
            if is_predefined_macro(&u.name)
                || patch_macros.contains(&u.name)
                || defs.macros.contains(u.name.as_str())
                || !flagged.insert(u.name.clone())
            {
                continue;
            }
            // a name `MacroName` refuses could not have entered `defs.macros`, so its absence there says nothing
            if MacroName::from_str(&u.name).is_err() {
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
                || defs.records.contains(u.name.as_str())
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

/// Whether the `-include` closure was read in full, and which side of
/// it a skipped header falls on. A first-party header can hide a
/// macro, a record, or a type; a skipped stdlib header can only hide a
/// macro or a record, since a module can only export a type declared
/// in its own text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IncludeCoverage {
    #[default]
    Complete,
    FirstPartyUnread,
    StdlibUnread,
    BothUnread,
}

impl IncludeCoverage {
    fn from_flags(first_party_unread: bool, stdlib_unread: bool) -> Self {
        match (first_party_unread, stdlib_unread) {
            (false, false) => Self::Complete,
            (true, false) => Self::FirstPartyUnread,
            (false, true) => Self::StdlibUnread,
            (true, true) => Self::BothUnread,
        }
    }

    /// True for every arm but `Complete`: an unread header of either
    /// kind can hide a macro or a record.
    #[must_use]
    pub fn hides_macros_or_records(self) -> bool {
        !matches!(self, Self::Complete)
    }

    /// True only when a first-party header went unread: a skipped
    /// stdlib header cannot hide a type a first-party module re-exports.
    #[must_use]
    pub fn hides_types(self) -> bool {
        matches!(self, Self::FirstPartyUnread | Self::BothUnread)
    }
}

/// Macros, records, and types defined in the target version of the
/// subject's file or a header it transitively includes, with the
/// include set taken from the target text and the patch's added text
/// alike.
#[derive(Debug, Default)]
pub struct TargetDefines {
    pub macros: BTreeSet<MacroName>,
    pub records: BTreeSet<RecordName>,
    pub types: BTreeSet<(TypeName, Arity)>,
    pub coverage: IncludeCoverage,
    /// A macro-expanded attribute form in the closure: any
    /// attribute-derived set here may be missing what it expands to.
    pub macro_attributes: bool,
}

/// Which side of the pick a header's text comes from.
enum HeaderSource {
    Target,
    PatchAdded,
}

struct IncludeWalk<'a> {
    macros: BTreeSet<MacroName>,
    records: BTreeSet<RecordName>,
    types: BTreeSet<(TypeName, Arity)>,
    macro_attributes: bool,
    first_party_unread: bool,
    stdlib_unread: bool,
    visited: BTreeSet<RelativePath>,
    stack: Vec<(RelativePath, usize, HeaderSource)>,
    patch_added: &'a BTreeMap<RelativePath, String>,
    target: &'a TargetTreeIndex,
}

impl IncludeWalk<'_> {
    fn absorb(&mut self, content: &str) {
        self.macros.extend(
            extract_defined_macros(content)
                .into_iter()
                .filter_map(|m| MacroName::new(m).ok()),
        );
        self.records.extend(
            extract_defined_records(content)
                .into_iter()
                .filter_map(|r| RecordName::new(r).ok()),
        );
        self.types.extend(extract_defined_types(content));
        self.macro_attributes |= has_macro_expanded_attribute(content);
    }

    fn follow_includes(&mut self, content: &str, from: &RelativePath, depth: usize) {
        for inc in extract_includes(content) {
            if is_stdlib_include_lib(&inc.directive) {
                self.stdlib_unread = true;
                continue;
            }
            // Target-first: a path present on both sides reads the target text.
            match resolve_include(self.target, from, &inc.directive) {
                Ok(resolved) => self.push(resolved, depth, HeaderSource::Target),
                Err(candidates) => {
                    let patch_hit = candidates
                        .into_iter()
                        .find(|c| self.patch_added.contains_key(c));
                    match patch_hit {
                        Some(path) => self.push(path, depth, HeaderSource::PatchAdded),
                        None => self.first_party_unread = true,
                    }
                }
            }
        }
    }

    fn push(&mut self, path: RelativePath, depth: usize, source: HeaderSource) {
        if !self.visited.insert(path.clone()) {
            return;
        }
        if depth > MAX_INCLUDE_DEPTH {
            self.first_party_unread = true;
            return;
        }
        self.stack.push((path, depth, source));
    }

    fn finish(self) -> TargetDefines {
        TargetDefines {
            macros: self.macros,
            records: self.records,
            types: self.types,
            coverage: IncludeCoverage::from_flags(self.first_party_unread, self.stdlib_unread),
            macro_attributes: self.macro_attributes,
        }
    }
}

/// `patch_added` holds the full text of every file the patch creates,
/// so an include that fails to resolve on the target can still resolve
/// against a header the same patch adds.
pub fn collect_target_defines(
    subject: &AddedLinesSubject<'_>,
    patch_added: &BTreeMap<RelativePath, String>,
    target: &TargetTreeIndex,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> TargetDefines {
    let mut walk = IncludeWalk {
        macros: BTreeSet::new(),
        records: BTreeSet::new(),
        types: BTreeSet::new(),
        macro_attributes: false,
        first_party_unread: false,
        stdlib_unread: false,
        visited: BTreeSet::new(),
        stack: Vec::new(),
        patch_added,
        target,
    };
    walk.visited.insert(subject.source_path.clone());
    // At the top file the includes to follow are the union of the target text's and the patch's added text's; an absent top file is the new-file case and stays complete.
    if let Some(content) = read_target(subject.source_path) {
        walk.absorb(&content);
        walk.follow_includes(&content, subject.source_path, 1);
    }
    walk.follow_includes(subject.added_text, subject.source_path, 1);
    while let Some((path, depth, source)) = walk.stack.pop() {
        let content = match source {
            HeaderSource::Target => read_target(&path),
            HeaderSource::PatchAdded => walk.patch_added.get(&path).cloned(),
        };
        let Some(content) = content else {
            walk.first_party_unread = true;
            continue;
        };
        walk.absorb(&content);
        walk.follow_includes(&content, &path, depth + 1);
    }
    walk.finish()
}

/// What the macro-value check produced: the drift reasons plus the
/// tally of what it did with each used name.
#[derive(Debug, Default)]
pub struct MacroValueAnalysis {
    pub reasons: Vec<Reason>,
    pub checks: MacroValueTally,
}

/// Compare the `-define` value of each macro the patch uses between
/// the two trees, when the definition is in the touched file itself
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
        // An absent top file is the new-file case: nothing pre-existing to compare; the existence axis owns the rest.
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
