// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves unqualified `f(Args)` calls a patch adds against the target
//! module's own function set: the case where a hunk calls a local
//! function a sibling commit introduced that the target branch lacks.
//!
//! Unlike macros and records, functions live in the `.erl` file (a
//! header almost never defines one), so this reads only the target
//! module, not its includes. To keep false positives near zero it is
//! conservative: it suppresses when the target file is unreadable or
//! declares a `parse_transform` (which injects unseeable functions),
//! and never flags an auto-imported BIF or an imported function.

use std::collections::BTreeSet;
use std::str::FromStr;

use crate::compat::source_attributes::{
    FunctionSignature, declares_parse_transform, extract_function_signatures, extract_imports,
};
use crate::model::names::{Arity, FunctionName, RelativePath};
use crate::model::verdict::Reason;

/// One touched `.erl` file: its path and the text of its added lines,
/// where new local calls appear.
#[derive(Debug, Clone, Copy)]
pub struct LocalCallSubject<'a> {
    pub source_path: &'a RelativePath,
    pub added_text: &'a str,
}

/// Flag each added unqualified call whose `f/arity` the target module
/// neither defines, imports, nor inherits as a BIF, and that the patch
/// does not define. One reason per `(file, function, arity)`.
pub fn analyse_local_calls(
    subjects: &[LocalCallSubject<'_>],
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> Vec<Reason> {
    let mut reasons = Vec::new();
    for subject in subjects {
        let sigs = extract_function_signatures(subject.added_text);
        if sigs.iter().all(|s| s.is_definition) {
            continue;
        }
        let Some(target) = read_target(subject.source_path) else {
            continue;
        };
        // A parse_transform injects functions the scanner cannot see.
        if declares_parse_transform(&target) {
            continue;
        }
        let patch_defs = defined_set(&sigs);
        let mut target_defs = defined_set(&extract_function_signatures(&target));
        target_defs.extend(extract_imports(&target));
        let mut flagged = BTreeSet::new();
        for call in sigs.iter().filter(|s| !s.is_definition) {
            let key = (call.name.clone(), call.arity);
            if is_auto_imported_bif(&call.name)
                || patch_defs.contains(&key)
                || target_defs.contains(&key)
                || !flagged.insert(key)
            {
                continue;
            }
            let (Ok(function), Ok(arity)) = (
                FunctionName::from_str(&call.name),
                u8::try_from(call.arity).map(Arity::new),
            ) else {
                continue;
            };
            reasons.push(Reason::LocalCallUndefinedOnTarget {
                source_path: subject.source_path.clone(),
                function,
                arity,
                line: call.line,
            });
        }
    }
    reasons
}

fn defined_set(sigs: &[FunctionSignature]) -> BTreeSet<(String, usize)> {
    sigs.iter()
        .filter(|s| s.is_definition)
        .map(|s| (s.name.clone(), s.arity))
        .collect()
}

/// Functions Erlang auto-imports from `erlang`, callable unqualified.
/// Matched by name across arities: conservative, so a BIF is never
/// flagged even at an arity not listed.
fn is_auto_imported_bif(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "apply"
            | "atom_to_binary"
            | "atom_to_list"
            | "binary_part"
            | "binary_to_atom"
            | "binary_to_existing_atom"
            | "binary_to_float"
            | "binary_to_integer"
            | "binary_to_list"
            | "binary_to_term"
            | "bit_size"
            | "bitstring_to_list"
            | "byte_size"
            | "ceil"
            | "date"
            | "element"
            | "erase"
            | "error"
            | "exit"
            | "float"
            | "float_to_binary"
            | "float_to_list"
            | "floor"
            | "garbage_collect"
            | "get"
            | "get_keys"
            | "group_leader"
            | "halt"
            | "hd"
            | "integer_to_binary"
            | "integer_to_list"
            | "iolist_size"
            | "iolist_to_binary"
            | "is_alive"
            | "is_atom"
            | "is_binary"
            | "is_bitstring"
            | "is_boolean"
            | "is_float"
            | "is_function"
            | "is_integer"
            | "is_list"
            | "is_map"
            | "is_map_key"
            | "is_number"
            | "is_pid"
            | "is_port"
            | "is_process_alive"
            | "is_record"
            | "is_reference"
            | "is_tuple"
            | "length"
            | "link"
            | "list_to_atom"
            | "list_to_binary"
            | "list_to_bitstring"
            | "list_to_existing_atom"
            | "list_to_float"
            | "list_to_integer"
            | "list_to_pid"
            | "list_to_tuple"
            | "make_ref"
            | "map_get"
            | "map_size"
            | "max"
            | "min"
            | "monitor"
            | "node"
            | "nodes"
            | "now"
            | "open_port"
            | "pid_to_list"
            | "process_flag"
            | "process_info"
            | "processes"
            | "put"
            | "register"
            | "registered"
            | "round"
            | "self"
            | "setelement"
            | "size"
            | "spawn"
            | "spawn_link"
            | "spawn_monitor"
            | "spawn_opt"
            | "split_binary"
            | "statistics"
            | "term_to_binary"
            | "throw"
            | "time"
            | "tl"
            | "trunc"
            | "tuple_size"
            | "tuple_to_list"
            | "unlink"
            | "unregister"
            | "whereis"
    )
}
