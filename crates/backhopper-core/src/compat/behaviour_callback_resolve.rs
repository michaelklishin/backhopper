// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Compares a declared behaviour's `-callback` surface between the two
//! trees: the case where a pick adds an implementation written against
//! the source's callback set and lands on a target whose set is wider
//! or shape-different. First-party behaviours only, the ones no pin
//! snapshot covers. A legacy behaviour that declares its surface
//! through `behaviour_info/1` has no `-callback` forms and is
//! invisible here, which withholds.

use std::collections::BTreeSet;

use crate::compat::added_lines::{AddedLinesSubject, file_line};
use crate::compat::qualified_call_resolve::{ModuleProvenance, TreeReader, classify_module};
use crate::compat::source_attributes::{
    extract_behaviours, extract_callbacks, extract_optional_callbacks,
};
use crate::model::names::{ModuleName, RelativePath};
use crate::model::verdict::Reason;

/// Flag callbacks the target's version of a declared behaviour
/// requires that the source's does not declare (an implementation
/// written against the source is missing them), and shared callbacks
/// whose normalized signatures disagree. Runs only for behaviours the
/// added lines declare; an unreadable side withholds silently, and a
/// callback the source requires but the target lacks is dead code, not
/// a backport risk. One pass per `(file, behaviour)`.
pub fn analyse_behaviour_callbacks(
    subjects: &[AddedLinesSubject<'_>],
    covered_modules: &BTreeSet<ModuleName>,
    resolve_module_path: &dyn Fn(&ModuleName) -> Option<RelativePath>,
    read_target: TreeReader<'_>,
    read_source: Option<TreeReader<'_>>,
) -> Vec<Reason> {
    let Some(read_source) = read_source else {
        return Vec::new();
    };
    let mut reasons = Vec::new();
    for subject in subjects {
        let mut seen = BTreeSet::new();
        for declared in extract_behaviours(subject.added_text) {
            if !seen.insert(declared.behaviour.clone()) {
                continue;
            }
            let ModuleProvenance::FirstParty { path } =
                classify_module(&declared.behaviour, covered_modules, resolve_module_path)
            else {
                continue;
            };
            let (Some(target_text), Some(source_text)) = (read_target(&path), read_source(&path))
            else {
                continue;
            };
            let target_callbacks = extract_callbacks(&target_text);
            if target_callbacks.is_empty() {
                continue;
            }
            let source_callbacks = extract_callbacks(&source_text);
            let optional = extract_optional_callbacks(&target_text);
            let line = file_line(subject.line_map, declared.line);
            for (key, target_signature) in &target_callbacks {
                match source_callbacks.get(key) {
                    None if !optional.contains(key) => {
                        reasons.push(Reason::BehaviourCallbackAddedOnTarget {
                            source_path: subject.source_path.clone(),
                            behaviour: declared.behaviour.clone(),
                            callback: key.0.clone(),
                            arity: key.1,
                            line,
                        });
                    }
                    Some(source_signature) if source_signature != target_signature => {
                        reasons.push(Reason::BehaviourCallbackDriftOnTarget {
                            source_path: subject.source_path.clone(),
                            behaviour: declared.behaviour.clone(),
                            callback: key.0.clone(),
                            arity: key.1,
                            source_signature: source_signature.clone(),
                            target_signature: target_signature.clone(),
                            line,
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    reasons
}
