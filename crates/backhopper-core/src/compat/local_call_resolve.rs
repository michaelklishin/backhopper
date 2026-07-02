// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves unqualified `f(Args)` calls a patch adds against the target
//! module's own function set: the case where a hunk calls a local
//! function a sibling commit introduced that the target branch lacks.
//! A call that does resolve against the module's own definitions gets
//! its `-spec` return shape compared between the checkouts, with the
//! same comparator and tally semantics as the qualified axis.
//!
//! Unlike macros and records, functions live in the `.erl` file (a
//! header almost never defines one), so this reads only the target
//! module, not its includes. To keep false positives near zero it is
//! conservative: it suppresses when the target file is unreadable or
//! declares a `parse_transform` (which injects unseeable functions),
//! and never flags an auto-imported BIF or an imported function. An
//! imported resolution is withheld from the shape check too: the
//! callee's `-spec` lives in another module.

use std::collections::BTreeSet;
use std::str::FromStr;

use crate::compat::added_lines::{AddedLinesSubject, file_line};
use crate::compat::qualified_call_resolve::{ShapeComparison, TreeReader, compare_return_shapes};
use crate::compat::source_attributes::{
    FunctionSignature, SpecTable, declares_parse_transform, extract_function_signatures,
    extract_imports, extract_specs,
};
use crate::model::names::{Arity, FunctionName};
use crate::model::verdict::{Reason, ShapeCheckTally};

/// What the local-call gate produced: the reasons plus the shape-check
/// tally, mirroring `QualifiedCallAnalysis`.
#[derive(Debug, Default)]
pub struct LocalCallAnalysis {
    pub reasons: Vec<Reason>,
    pub shape_checks: ShapeCheckTally,
}

/// Flag each added unqualified call whose `f/arity` the target module
/// neither defines, imports, nor inherits as a BIF, and that the patch
/// does not define; compare the `-spec` return shape of each call the
/// target module itself defines. One reason per `(file, function,
/// arity)`.
pub fn analyse_local_calls(
    subjects: &[AddedLinesSubject<'_>],
    read_target: TreeReader<'_>,
    read_source: Option<TreeReader<'_>>,
) -> LocalCallAnalysis {
    let mut analysis = LocalCallAnalysis::default();
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
        // A spec the patch rewrites lands on the target with the pick:
        // no pre-existing drift to compare.
        let patch_specs = extract_specs(subject.added_text);
        let target_defined = defined_set(&extract_function_signatures(&target));
        let imported = extract_imports(&target);
        // The callee module is the subject file itself: one table pair
        // per subject, filled on the first resolved call.
        let mut spec_tables: Option<(SpecTable, Option<SpecTable>)> = None;
        let mut flagged = BTreeSet::new();
        let mut shape_seen = BTreeSet::new();
        for call in sigs.iter().filter(|s| !s.is_definition) {
            let key = (call.name.clone(), call.arity);
            if is_auto_imported_bif(&call.name) || patch_defs.contains(&key) {
                continue;
            }
            if target_defined.contains(&key) {
                if !shape_seen.insert(key) {
                    continue;
                }
                check_local_return_shape(
                    subject,
                    &target,
                    call,
                    &patch_specs,
                    read_source,
                    &mut spec_tables,
                    &mut analysis,
                );
                continue;
            }
            if imported.contains(&key) {
                if shape_seen.insert(key) {
                    analysis.shape_checks.withheld_imported += 1;
                }
                continue;
            }
            if !flagged.insert(key) {
                continue;
            }
            let (Ok(function), Ok(arity)) = (
                FunctionName::from_str(&call.name),
                Arity::try_from(call.arity),
            ) else {
                continue;
            };
            analysis.reasons.push(Reason::LocalCallUndefinedOnTarget {
                source_path: subject.source_path.clone(),
                function,
                arity,
                line: file_line(subject.line_map, call.line),
            });
        }
    }
    analysis
}

fn check_local_return_shape(
    subject: &AddedLinesSubject<'_>,
    target_text: &str,
    call: &FunctionSignature,
    patch_specs: &SpecTable,
    read_source: Option<TreeReader<'_>>,
    spec_tables: &mut Option<(SpecTable, Option<SpecTable>)>,
    analysis: &mut LocalCallAnalysis,
) {
    let tally = &mut analysis.shape_checks;
    let (Ok(function), Ok(arity)) = (
        FunctionName::from_str(&call.name),
        Arity::try_from(call.arity),
    ) else {
        return;
    };
    let key = (function, arity);
    if patch_specs.contains_key(&key) {
        return;
    }
    let Some(read_source) = read_source else {
        tally.withheld_no_source += 1;
        return;
    };
    let (target_specs, source_specs) = spec_tables.get_or_insert_with(|| {
        (
            extract_specs(target_text),
            read_source(subject.source_path).map(|t| extract_specs(&t)),
        )
    });
    let Some(source_specs) = source_specs.as_ref() else {
        tally.withheld_no_source += 1;
        return;
    };
    match compare_return_shapes(target_specs, source_specs, &key) {
        ShapeComparison::Same => tally.compared += 1,
        ShapeComparison::NoSpec => tally.withheld_no_spec += 1,
        ShapeComparison::UnknownType => tally.withheld_unknown_type += 1,
        ShapeComparison::Drift {
            source_signature,
            target_signature,
        } => {
            tally.compared += 1;
            analysis.reasons.push(Reason::LocalCallReturnShapeDrift {
                source_path: subject.source_path.clone(),
                function: key.0,
                arity: key.1,
                source_signature,
                target_signature,
                line: file_line(subject.line_map, call.line),
            });
        }
    }
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
