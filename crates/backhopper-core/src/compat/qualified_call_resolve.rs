// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves qualified `m:f/a` calls a patch adds against the called
//! module's exports on the target tree: the snapshot-free counterpart
//! of `local_call_resolve` for the qualified case. A call is flagged
//! only when its module is first-party (present on the target tree and
//! covered by no pin snapshot), the export set is fully readable, and
//! neither the target nor the patch exports `f/a`. Everything else is
//! withheld and left to the build.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use crate::compat::added_lines::file_line;
use crate::compat::call_sites::extract_qualified_calls;
use crate::compat::source_attributes::{ExportSet, extract_exports, extract_function_signatures};
use crate::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use crate::model::verdict::Reason;

/// One touched `.erl` file: its path, the text of its added lines where
/// qualified calls appear, and the blob-line to file-line map for the
/// reported line.
#[derive(Debug, Clone, Copy)]
pub struct QualifiedCallSubject<'a> {
    pub source_path: &'a RelativePath,
    pub added_text: &'a str,
    pub line_map: &'a [u32],
}

/// `(function, arity)` pairs the patch adds per module: definitions and
/// `-export` entries. A qualified call to one of these is not flagged,
/// since the patch introduces it (often in a different file than the
/// call).
pub type PatchAddedFunctions = BTreeMap<ModuleName, BTreeSet<(FunctionName, Arity)>>;

/// Where a called module sits relative to this axis. Only `FirstParty`
/// is resolved; the other two are withheld, so a dependency call or an
/// OTP call cannot be flagged by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleProvenance {
    /// A pin snapshot (tracked dep or self-pin) already resolves this
    /// module's calls: defer to the snapshot axis.
    CoveredBySnapshot,
    /// A first-party `.erl` module present on the target tree, covered
    /// by no snapshot: this axis resolves it live against its exports.
    FirstParty { path: RelativePath },
    /// Covered by no snapshot and absent from the tree (OTP, stdlib, or
    /// a genuinely-missing module): withhold, the build is the backstop.
    Unknown,
}

/// Classify a called module. Snapshot coverage wins over an in-tree
/// file, so a tracked dep that also has a vendored copy in the tree
/// still defers to its snapshot.
pub fn classify_module(
    module: &ModuleName,
    covered_modules: &BTreeSet<ModuleName>,
    resolve_module_path: &dyn Fn(&ModuleName) -> Option<RelativePath>,
) -> ModuleProvenance {
    if covered_modules.contains(module) {
        return ModuleProvenance::CoveredBySnapshot;
    }
    match resolve_module_path(module) {
        Some(path) => ModuleProvenance::FirstParty { path },
        None => ModuleProvenance::Unknown,
    }
}

/// Functions the patch adds per module, gathered across every touched
/// `.erl` file so a cross-file add (call in one file, callee and its
/// `-export` in the module's own file) does not false-positive.
pub fn patch_added_functions(per_file: &[(ModuleName, &str)]) -> PatchAddedFunctions {
    let mut out: PatchAddedFunctions = BTreeMap::new();
    for (module, added_text) in per_file {
        let entry = out.entry(module.clone()).or_default();
        for sig in extract_function_signatures(added_text) {
            if !sig.is_definition {
                continue;
            }
            if let (Ok(function), Ok(arity)) =
                (FunctionName::from_str(&sig.name), u8::try_from(sig.arity))
            {
                entry.insert((function, Arity::new(arity)));
            }
        }
        entry.extend(extract_exports(added_text).exports);
    }
    out
}

/// Flag each added qualified call whose module is first-party on the
/// target and does not export the called `f/a`. `resolve_module_path`
/// maps a module to its `.erl` path on the target tree (the first-party
/// gate: `None` means absent, so withhold); `read_target` reads that
/// path. `covered_modules` is the union of every pin snapshot's
/// modules: a call into one of them is the snapshot axis's, not this
/// one's. One reason per `(file, module, function, arity)`.
pub fn analyse_qualified_calls(
    subjects: &[QualifiedCallSubject<'_>],
    covered_modules: &BTreeSet<ModuleName>,
    patch_added: &PatchAddedFunctions,
    resolve_module_path: &dyn Fn(&ModuleName) -> Option<RelativePath>,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> Vec<Reason> {
    let mut reasons = Vec::new();
    // None caches a withheld module (covered, absent, or unreadable
    // export surface), so a second call into it skips the work too.
    let mut exports_by_module: BTreeMap<ModuleName, Option<ExportSet>> = BTreeMap::new();
    for subject in subjects {
        let calls = extract_qualified_calls(subject.added_text);
        if calls.is_empty() {
            continue;
        }
        let mut flagged: BTreeSet<(ModuleName, FunctionName, Arity)> = BTreeSet::new();
        for call in calls {
            let module = &call.mfa.module;
            let exports = exports_by_module.entry(module.clone()).or_insert_with(|| {
                match classify_module(module, covered_modules, resolve_module_path) {
                    ModuleProvenance::FirstParty { path } => {
                        let exports = extract_exports(&read_target(&path)?);
                        exports.complete.then_some(exports)
                    }
                    ModuleProvenance::CoveredBySnapshot | ModuleProvenance::Unknown => None,
                }
            });
            let Some(exports) = exports.as_ref() else {
                continue;
            };
            let key = (call.mfa.function.clone(), call.mfa.arity);
            if exports.exports.contains(&key)
                || patch_added.get(module).is_some_and(|s| s.contains(&key))
            {
                continue;
            }
            if !flagged.insert((module.clone(), call.mfa.function.clone(), call.mfa.arity)) {
                continue;
            }
            reasons.push(Reason::QualifiedCallUndefinedOnTarget {
                source_path: subject.source_path.clone(),
                module: module.clone(),
                function: call.mfa.function,
                arity: call.mfa.arity,
                line: file_line(subject.line_map, call.line),
            });
        }
    }
    reasons
}
