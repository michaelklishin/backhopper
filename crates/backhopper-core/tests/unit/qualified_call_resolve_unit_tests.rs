// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_qualified_calls` against injected module-path and
//! target-reader closures: the live target-tree resolver for qualified
//! `m:f/a` calls, and the return-shape check over resolved ones.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::qualified_call_resolve::{
    PatchProvided, QualifiedCallAnalysis, analyse_qualified_calls, patch_provided,
};
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use backhopper_core::model::verdict::{Reason, ShapeCheckTally};

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

fn drifted(reasons: &[Reason]) -> Vec<(String, String, String, u32)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::QualifiedCallReturnShapeDrift {
                function,
                source_signature,
                target_signature,
                line,
                ..
            } => Some((
                function.to_string(),
                source_signature.clone(),
                target_signature.clone(),
                *line,
            )),
            _ => None,
        })
        .collect()
}

/// `target` rows are `(module, path, file text)`; `source` rows are
/// `(path, file text)` for the source-checkout side, `None` meaning no
/// checkout is available.
fn analyse_full(
    added: &str,
    line_map: &[u32],
    covered: &[&str],
    patch_added: &PatchProvided,
    target: &[(&str, &str, &str)],
    source: Option<&[(&str, &str)]>,
) -> QualifiedCallAnalysis {
    let path = rp("deps/some-app/src/caller.erl");
    // A bare `&[]` means "contiguous": synthesize the 1..=n identity
    // map the extractor's length assert requires.
    let seq: Vec<u32>;
    let line_map = if line_map.is_empty() && !added.is_empty() {
        seq = (1..=added.lines().count() as u32).collect();
        &seq
    } else {
        line_map
    };
    let subjects = [AddedLinesSubject {
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
    let source_map: Option<BTreeMap<String, String>> = source.map(|rows| {
        rows.iter()
            .map(|(p, s)| ((*p).to_owned(), (*s).to_owned()))
            .collect()
    });
    let covered: BTreeSet<ModuleName> = covered.iter().map(|s| module(s)).collect();
    let resolve = |m: &ModuleName| module_to_path.get(m).cloned();
    let read = |p: &RelativePath| path_to_src.get(p.as_str()).cloned();
    let read_source_fn = source_map
        .as_ref()
        .map(|m| move |p: &RelativePath| m.get(p.as_str()).cloned());
    let read_source = read_source_fn
        .as_ref()
        .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
    analyse_qualified_calls(
        &subjects,
        &covered,
        patch_added,
        &resolve,
        &read,
        read_source,
    )
}

fn analyse_with(
    added: &str,
    line_map: &[u32],
    covered: &[&str],
    patch_added: &PatchProvided,
    target: &[(&str, &str, &str)],
) -> Vec<Reason> {
    analyse_full(added, line_map, covered, patch_added, target, None).reasons
}

fn analyse(added: &str, target: &[(&str, &str, &str)]) -> Vec<Reason> {
    analyse_with(added, &[], &[], &PatchProvided::default(), target)
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
        &PatchProvided::default(),
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
            "deps/some-app/src/other.erl",
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
            "deps/some-app/src/other.erl",
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
            "deps/some-app/src/other.erl",
            "-module(other).\n-compile({parse_transform, t}).\n-export([known/0]).\n",
        )],
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

// A pick that adds both the call and the callee (in the callee's own
// file) must not false-positive: the patch-wide added map covers it.
#[test]
fn a_function_the_patch_adds_cross_file_is_not_flagged() {
    let mut patch_added = PatchProvided::default();
    patch_added.functions.insert(
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
            "deps/some-app/src/other.erl",
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
        &PatchProvided::default(),
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\n",
        )],
    );
    assert_eq!(flagged(&reasons)[0].3, 120);
}

#[test]
fn patch_provided_gathers_definitions_exports_and_specs() {
    let added_a = "-export([exported/1]).\nexported(X) -> X.\n";
    let added_b = "-spec defined(term(), term()) -> tuple().\ndefined(A, B) -> {A, B}.\n";
    let per_file = [(module("m"), added_a), (module("m"), added_b)];
    let provided = patch_provided(&per_file);
    let functions = provided.functions.get(&module("m")).unwrap();
    assert!(functions.contains(&(FunctionName::from_str("exported").unwrap(), Arity::new(1))));
    assert!(functions.contains(&(FunctionName::from_str("defined").unwrap(), Arity::new(2))));
    let specs = provided.specs.get(&module("m")).unwrap();
    assert!(specs.contains(&(FunctionName::from_str("defined").unwrap(), Arity::new(2))));
}

// Return-shape drift over resolved calls

const IDX_PATH: &str = "deps/rabbit/src/rabbit_classic_queue_index_v2.erl";

fn idx_target(spec: &str) -> Vec<(&'static str, &'static str, String)> {
    vec![(
        "rabbit_classic_queue_index_v2",
        IDX_PATH,
        format!(
            "-module(rabbit_classic_queue_index_v2).\n-export([info/1]).\n{spec}info(S) -> S.\n"
        ),
    )]
}

fn analyse_shapes(added: &str, target_spec: &str, source_spec: &str) -> QualifiedCallAnalysis {
    let target_rows = idx_target(target_spec);
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let source_text = format!(
        "-module(rabbit_classic_queue_index_v2).\n-export([info/1]).\n{source_spec}info(S) -> S.\n"
    );
    let source_rows = [(IDX_PATH, source_text.as_str())];
    analyse_full(
        added,
        &[],
        &[],
        &PatchProvided::default(),
        &target,
        Some(&source_rows),
    )
}

#[test]
fn a_drifted_return_shape_is_flagged_with_both_signatures() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert_eq!(
        drifted(&analysis.reasons),
        [(
            "info".to_owned(),
            "info(state()) -> list()".to_owned(),
            "info(state()) -> binary()".to_owned(),
            1,
        )]
    );
    assert_eq!(analysis.shape_checks.compared, 1);
}

#[test]
fn the_drift_reason_is_non_blocking() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert!(analysis.reasons.iter().all(|r| !r.is_blocking()));
}

#[test]
fn the_same_shape_modulo_whitespace_is_clean() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) ->\n    binary().\n",
        "-spec info(state()) -> binary().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.compared, 1);
}

#[test]
fn reordered_union_members_are_clean() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) -> binary() | undefined.\n",
        "-spec info(state()) -> undefined | binary().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.compared, 1);
}

#[test]
fn a_missing_target_spec_withholds_and_is_counted() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "",
        "-spec info(state()) -> list().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_no_spec, 1);
    assert_eq!(analysis.shape_checks.compared, 0);
}

#[test]
fn a_missing_source_spec_withholds_and_is_counted() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) -> binary().\n",
        "",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_no_spec, 1);
}

#[test]
fn an_unmodelled_return_type_withholds_and_is_counted() {
    // A literal bitstring type is a construct the return parser does
    // not model, so it parses to SpecType::Unknown.
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) -> <<_:8, _:_*8>>.\n",
        "-spec info(state()) -> list().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_unknown_type, 1);
    assert_eq!(analysis.shape_checks.compared, 0);
}

#[test]
fn no_source_checkout_withholds_each_resolved_call_once() {
    let target_rows = idx_target("-spec info(state()) -> binary().\n");
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let analysis = analyse_full(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\ng(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        &[],
        &[],
        &PatchProvided::default(),
        &target,
        None,
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_no_source, 1);
}

#[test]
fn the_same_drifted_call_twice_in_one_file_yields_one_reason() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\ng(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert_eq!(drifted(&analysis.reasons).len(), 1);
    assert_eq!(analysis.shape_checks.compared, 1);
}

#[test]
fn an_undefined_call_gets_no_shape_check() {
    // Export resolution already failed: the shape check never runs, so
    // the tally stays empty and only the undefined reason fires.
    let analysis = analyse_full(
        "f(V, Q) -> rabbit_misc:queue_resource(V, Q).\n",
        &[],
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\n-spec queue_resource(term(), term()) -> tuple().\n",
        )],
        Some(&[(
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-spec queue_resource(term(), term()) -> map().\n",
        )]),
    );
    assert_eq!(flagged(&analysis.reasons).len(), 1);
    assert_eq!(drifted(&analysis.reasons).len(), 0);
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

#[test]
fn a_patch_added_callee_gets_no_shape_check() {
    let mut patch_added = PatchProvided::default();
    patch_added.functions.insert(
        module("rabbit_classic_queue_index_v2"),
        BTreeSet::from([(FunctionName::from_str("info").unwrap(), Arity::new(1))]),
    );
    let target_rows = idx_target("-spec info(state()) -> binary().\n");
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let analysis = analyse_full(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        &[],
        &[],
        &patch_added,
        &target,
        Some(&[(IDX_PATH, "-spec info(state()) -> list().\n")]),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

#[test]
fn covered_and_unknown_modules_get_no_shape_check() {
    let analysis = analyse_full(
        "f(X) -> ra:add_member(X), lists:flatten(X).\n",
        &[],
        &["ra"],
        &PatchProvided::default(),
        &[(
            "ra",
            "deps/ra/src/ra.erl",
            "-module(ra).\n-export([add_member/1]).\n-spec add_member(term()) -> ok.\n",
        )],
        Some(&[(
            "deps/ra/src/ra.erl",
            "-module(ra).\n-spec add_member(term()) -> {ok, term()}.\n",
        )]),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

#[test]
fn the_drift_line_is_translated_through_the_map() {
    let target_rows = idx_target("-spec info(state()) -> binary().\n");
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let analysis = analyse_full(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        &[240],
        &[],
        &PatchProvided::default(),
        &target,
        Some(&[(IDX_PATH, "-spec info(state()) -> list().\n")]),
    );
    assert_eq!(drifted(&analysis.reasons)[0].3, 240);
}

#[test]
fn a_patch_rewritten_spec_gets_no_shape_check() {
    // The pick carries the new spec to the target, so the pre-existing
    // difference between the trees is not drift the pick will hit.
    let mut patch_added = PatchProvided::default();
    patch_added.specs.insert(
        module("rabbit_classic_queue_index_v2"),
        BTreeSet::from([(FunctionName::from_str("info").unwrap(), Arity::new(1))]),
    );
    let target_rows = idx_target("-spec info(state()) -> binary().\n");
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let analysis = analyse_full(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        &[],
        &[],
        &patch_added,
        &target,
        Some(&[(IDX_PATH, "-spec info(state()) -> list().\n")]),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

#[test]
fn an_unreadable_source_module_counts_as_no_source_not_no_spec() {
    // A checkout without the callee's file (e.g. a defaulted
    // `--repo-dir-path` pointing somewhere else) is a missing source,
    // not a readable module that happens to lack a spec.
    let target_rows = idx_target("-spec info(state()) -> binary().\n");
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let analysis = analyse_full(
        "f(S) -> rabbit_classic_queue_index_v2:info(S).\n",
        &[],
        &[],
        &PatchProvided::default(),
        &target,
        Some(&[]),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_no_source, 1);
    assert_eq!(analysis.shape_checks.withheld_no_spec, 0);
}

// Joined runs (046 §A): wrapped calls reach the gate

#[test]
fn a_wrapped_undefined_call_is_flagged_with_exact_arity() {
    let reasons = analyse(
        "f(V, Q) -> rabbit_misc:queue_resource(V,\n    Q).\n",
        &[(
            "rabbit_misc",
            "deps/rabbit/src/rabbit_misc.erl",
            "-module(rabbit_misc).\n-export([r/3]).\n",
        )],
    );
    assert_eq!(
        flagged(&reasons),
        [("rabbit_misc".to_owned(), "queue_resource".to_owned(), 2, 1)]
    );
}

#[test]
fn a_wrapped_resolved_call_is_shape_compared() {
    let analysis = analyse_shapes(
        "f(S) -> rabbit_classic_queue_index_v2:info(\n    S).\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert_eq!(drifted(&analysis.reasons).len(), 1);
    assert_eq!(analysis.shape_checks.compared, 1);
}

#[test]
fn repro_e2e_line_attribution_through_the_gate() {
    let target_rows = idx_target("-spec info(state()) -> binary().\n");
    let target: Vec<(&str, &str, &str)> = target_rows
        .iter()
        .map(|(m, p, s)| (*m, *p, s.as_str()))
        .collect();
    let analysis = analyse_full(
        "t_info(S) -> rabbit_classic_queue_index_v2:info(S).\nt_segment(Seg, Dir) -> rabbit_classic_queue_index_v2:segment_file(Seg, Dir).\nt_local(S) -> local_info(S).\n",
        &[6, 7, 8],
        &[],
        &PatchProvided::default(),
        &target,
        Some(&[(IDX_PATH, "-spec info(state()) -> list().\n")]),
    );
    assert_eq!(drifted(&analysis.reasons)[0].3, 6);
}
