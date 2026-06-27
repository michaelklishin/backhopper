// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_qualified_calls` against injected module-path and
//! target-reader closures: the live target-tree resolver for qualified
//! `m:f/a` calls.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::qualified_call_resolve::{
    PatchAddedFunctions, QualifiedCallSubject, analyse_qualified_calls, patch_added_functions,
};
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use backhopper_core::model::verdict::Reason;

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn flagged(reasons: &[Reason]) -> Vec<(String, String, u8, u32)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::QualifiedCallUndefinedOnTarget {
                module,
                function,
                arity,
                line,
                ..
            } => Some((module.to_string(), function.to_string(), arity.get(), *line)),
            _ => None,
        })
        .collect()
}

fn analyse_with(
    added: &str,
    line_map: &[u32],
    covered: &[&str],
    patch_added: &PatchAddedFunctions,
    target: &[(&str, &str, &str)],
) -> Vec<Reason> {
    let path = rp("deps/rabbit/src/caller.erl");
    let subjects = [QualifiedCallSubject {
        source_path: &path,
        added_text: added,
        line_map,
    }];
    let module_to_path: BTreeMap<ModuleName, RelativePath> =
        target.iter().map(|(m, p, _)| (module(m), rp(p))).collect();
    let path_to_src: BTreeMap<String, String> = target
        .iter()
        .map(|(_, p, s)| ((*p).to_owned(), (*s).to_owned()))
        .collect();
    let covered: BTreeSet<ModuleName> = covered.iter().map(|s| module(s)).collect();
    let resolve = |m: &ModuleName| module_to_path.get(m).cloned();
    let read = |p: &RelativePath| path_to_src.get(p.as_str()).cloned();
    analyse_qualified_calls(&subjects, &covered, patch_added, &resolve, &read)
}

fn analyse(added: &str, target: &[(&str, &str, &str)]) -> Vec<Reason> {
    analyse_with(added, &[], &[], &PatchAddedFunctions::new(), target)
}

// A first-party module present on target that lacks the called export
// is flagged. The #16771 witness: rabbit_misc exports r/3, not
// queue_resource/2.
#[test]
fn a_qualified_call_absent_on_target_is_flagged() {
    let reasons = analyse(
        "f(V, Q) -> rabbit_misc:queue_resource(V, Q).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\nr(A, B, C) -> {A, B, C}.\n",
        )],
    );
    assert_eq!(
        flagged(&reasons),
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn a_qualified_call_exported_on_target_is_clean() {
    let reasons = analyse(
        "f(V, Q) -> rabbit_misc:queue_resource(V, Q).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([queue_resource/2]).\nqueue_resource(A, B) -> {A, B}.\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

// Resolution is against the exports, not the defined set: a function
// the target module defines but does not export is a runtime undef on a
// qualified call, so it is flagged.
#[test]
fn a_defined_but_unexported_function_is_flagged() {
    let reasons = analyse(
        "f() -> rabbit_misc:internal().\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\ninternal() -> ok.\n",
        )],
    );
    assert_eq!(
        flagged(&reasons),
        [("rabbit_misc".to_owned(), "internal".to_owned(), 0, 1)]
    );
}

// A module a pin snapshot covers defers to the snapshot axis, even when
// the in-tree copy lacks the export: the deps/ra-vs-deps/rabbit hazard.
#[test]
fn a_snapshot_covered_module_is_not_flagged() {
    let reasons = analyse_with(
        "f(X) -> ra:never_exported(X).\n",
        &[],
        &["ra"],
        &PatchAddedFunctions::new(),
        &[(
            "ra",
            "deps/ra/src/ra.erl",
            "-module(ra).\n-export([start/0]).\nstart() -> ok.\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_module_absent_from_the_tree_is_not_flagged() {
    let reasons = analyse("f(X) -> lists:flatten(X).\n", &[]);
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

// export_all means every defined function is exported; the listed
// exports under-report, so the resolver withholds.
#[test]
fn an_export_all_module_is_withheld() {
    let reasons = analyse(
        "f(X) -> other:helper(X).\n",
        &[(
            "other",
            "deps/rabbit/src/other.erl",
            "-module(other).\n-compile(export_all).\nhelper(X) -> X.\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_macro_in_the_export_list_withholds() {
    let reasons = analyse(
        "f(X) -> other:helper(X).\n",
        &[(
            "other",
            "deps/rabbit/src/other.erl",
            "-module(other).\n-export([?API]).\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_parse_transform_module_is_withheld() {
    let reasons = analyse(
        "f(X) -> other:injected(X).\n",
        &[(
            "other",
            "deps/rabbit/src/other.erl",
            "-module(other).\n-compile({parse_transform, t}).\n-export([known/0]).\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

// A pick that adds both the call and the callee (in the callee's own
// file) must not false-positive: the patch-wide added map covers it.
#[test]
fn a_function_the_patch_adds_cross_file_is_not_flagged() {
    let mut patch_added = PatchAddedFunctions::new();
    patch_added.insert(
        module("other"),
        BTreeSet::from([(FunctionName::from_str("new_fn").unwrap(), Arity::new(1))]),
    );
    let reasons = analyse_with(
        "f(X) -> other:new_fn(X).\n",
        &[],
        &[],
        &patch_added,
        &[(
            "other",
            "deps/rabbit/src/other.erl",
            "-module(other).\n-export([old/0]).\nold() -> ok.\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

// The load-bearing test: a readable module with one exported and one
// unexported call flags only the unexported one. Distinguishes
// "narrowed correctly" from "silently disabled".
#[test]
fn preservation_flags_only_the_unexported_call() {
    let reasons = analyse(
        "f(A, B, C) -> rabbit_misc:r(A, B, C), rabbit_misc:queue_resource(A, B).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\nr(A, B, C) -> {A, B, C}.\n",
        )],
    );
    assert_eq!(
        flagged(&reasons),
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn arity_distinguishes_the_call() {
    let reasons = analyse(
        "f(A, B) -> rabbit_misc:queue_resource(A, B).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([queue_resource/3]).\nqueue_resource(A, B, C) -> {A, B, C}.\n",
        )],
    );
    assert_eq!(
        flagged(&reasons),
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn two_distinct_undefined_functions_yield_two_findings() {
    let reasons = analyse(
        "f(X) -> rabbit_misc:alpha(X), rabbit_misc:beta(X).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\n",
        )],
    );
    let mut got: Vec<String> = flagged(&reasons)
        .into_iter()
        .map(|(_, f, _, _)| f)
        .collect();
    got.sort();
    assert_eq!(got, ["alpha", "beta"]);
}

#[test]
fn two_calls_to_the_same_undefined_function_yield_one_finding() {
    let reasons = analyse(
        "f(Q) -> rabbit_misc:queue_resource(a, Q).\ng(Q) -> rabbit_misc:queue_resource(b, Q).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\n",
        )],
    );
    assert_eq!(flagged(&reasons).len(), 1);
}

// A qualified type in a -spec is not a call and must not be flagged.
#[test]
fn a_qualified_type_in_a_spec_is_not_flagged() {
    let reasons = analyse(
        "-spec f(othermod:t()) -> ok.\nf(_) -> ok.\n",
        &[(
            "othermod",
            "deps/rabbit/src/othermod.erl",
            "-module(othermod).\n-export([g/0]).\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn the_reported_line_is_translated_through_the_map() {
    let reasons = analyse_with(
        "f(V, Q) -> rabbit_misc:queue_resource(V, Q).\n",
        &[120],
        &[],
        &PatchAddedFunctions::new(),
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\n",
        )],
    );
    assert_eq!(flagged(&reasons)[0].3, 120);
}

#[test]
fn patch_added_functions_gathers_definitions_and_exports() {
    let added_a = "-export([exported/1]).\nexported(X) -> X.\n";
    let added_b = "defined(A, B) -> {A, B}.\n";
    let per_file = [(module("m"), added_a), (module("m"), added_b)];
    let map = patch_added_functions(&per_file);
    let set = map.get(&module("m")).unwrap();
    assert!(set.contains(&(FunctionName::from_str("exported").unwrap(), Arity::new(1))));
    assert!(set.contains(&(FunctionName::from_str("defined").unwrap(), Arity::new(2))));
}
