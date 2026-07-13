// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_local_calls` against injected target-module readers. A
//! call the patch imports beside its first use is resolved as the
//! cross-module reference it is, so the harness also serves the
//! imported module's file and maps module names to paths.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::local_call_resolve::{LocalCallAnalysis, analyse_local_calls};
use backhopper_core::compat::qualified_call_resolve::{
    PatchProvided, ReferenceContext, patch_provided,
};
use backhopper_core::model::names::{ModuleName, RelativePath};
use backhopper_core::model::verdict::{Reason, ShapeCheckTally};

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn module_of_path(p: &str) -> ModuleName {
    let stem = p
        .rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".erl"))
        .unwrap();
    ModuleName::from_str(stem).unwrap()
}

fn flagged(reasons: &[Reason]) -> Vec<(&str, u8)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::LocalCallUndefinedOnTarget {
                function, arity, ..
            } => Some((function.as_str(), arity.get())),
            _ => None,
        })
        .collect()
}

fn qualified_flagged(reasons: &[Reason]) -> Vec<(String, String, u8)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::QualifiedCallUndefinedOnTarget {
                module,
                function,
                arity,
                ..
            } => Some((module.to_string(), function.to_string(), arity.get())),
            _ => None,
        })
        .collect()
}

/// `target` rows are `(path, file text)`; the module a path names is
/// its `.erl` stem, so an imported module resolves to its own row.
fn analyse_ctx(
    path: &RelativePath,
    added: &str,
    covered: &[&str],
    patch_added: &PatchProvided,
    target: &[(&str, &str)],
    source: Option<&[(&str, &str)]>,
) -> LocalCallAnalysis {
    let subjects = [AddedLinesSubject {
        source_path: path,
        added_text: added,
        line_map: &[],
    }];
    let target_map: BTreeMap<String, String> = target
        .iter()
        .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
        .collect();
    let module_to_path: BTreeMap<ModuleName, RelativePath> = target
        .iter()
        .map(|(p, _)| (module_of_path(p), rp(p)))
        .collect();
    let source_map: Option<BTreeMap<String, String>> = source.map(|rows| {
        rows.iter()
            .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
            .collect()
    });
    let covered: BTreeSet<ModuleName> = covered
        .iter()
        .map(|s| ModuleName::from_str(s).unwrap())
        .collect();
    let resolve = |m: &ModuleName| module_to_path.get(m).cloned();
    let read = |p: &RelativePath| target_map.get(p.as_str()).cloned();
    let read_source_fn = source_map
        .as_ref()
        .map(|m| move |p: &RelativePath| m.get(p.as_str()).cloned());
    let read_source = read_source_fn
        .as_ref()
        .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
    let ctx = ReferenceContext {
        covered_modules: &covered,
        patch_added,
        resolve_module_path: &resolve,
        read_target: &read,
        read_source,
    };
    analyse_local_calls(&subjects, &ctx)
}

fn analyse_full(
    path: &RelativePath,
    added: &str,
    target: &[(&str, &str)],
    source: Option<&[(&str, &str)]>,
) -> LocalCallAnalysis {
    analyse_ctx(path, added, &[], &PatchProvided::default(), target, source)
}

fn analyse(path: &RelativePath, added: &str, target: &[(&str, &str)]) -> Vec<Reason> {
    analyse_full(path, added, target, None).reasons
}

#[test]
fn a_call_to_a_function_absent_on_target_is_flagged() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(P) -> reader_options(P).\n",
        &[("deps/rabbit/src/r.erl", "-module(r).\ng(P) -> ok.\n")],
    );
    assert_eq!(flagged(&reasons), [("reader_options", 1)]);
}

#[test]
fn a_call_to_a_function_defined_on_target_is_clean() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(P) -> reader_options(P).\n",
        &[(
            "deps/rabbit/src/r.erl",
            "-module(r).\nreader_options(X) -> X.\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn an_auto_imported_bif_is_never_flagged() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(L) -> length(L).\n",
        &[("deps/rabbit/src/r.erl", "-module(r).\n")],
    );
    assert!(reasons.is_empty());
}

#[test]
fn an_imported_function_is_never_flagged() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(X) -> helper(X).\n",
        &[(
            "deps/rabbit/src/r.erl",
            "-module(r).\n-import(other, [helper/1]).\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn a_function_the_patch_defines_is_clean() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(P) -> helper(P).\nhelper(X) -> X.\n",
        &[("deps/rabbit/src/r.erl", "-module(r).\n")],
    );
    assert!(reasons.is_empty());
}

// A parse_transform can inject functions the scanner cannot see, so the
// whole module is suppressed.
#[test]
fn a_parse_transform_suppresses_flagging() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(P) -> injected(P).\n",
        &[(
            "deps/rabbit/src/r.erl",
            "-module(r).\n-compile({parse_transform, lager_transform}).\n",
        )],
    );
    assert!(reasons.is_empty());
}

// Arity is part of the key: a call to f/2 is undefined even when f/1
// exists on target.
#[test]
fn arity_distinguishes_the_call() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g(A, B) -> f(A, B).\n",
        &[("deps/rabbit/src/r.erl", "-module(r).\nf(X) -> X.\n")],
    );
    assert_eq!(flagged(&reasons), [("f", 2)]);
}

// Regression from rabbitmq/rabbitmq-server #16736 (commit bd457076ac):
// a clean-applying backport was flagged because the scanner read a
// variable application (`Fun(...)`), an `-export` form, and `-spec` type
// names as undefined local calls. None are calls; the only real call,
// `queue_definition/1`, is defined on target, so the result is clean.
#[test]
fn rabbitmq_definitions_backport_flags_no_spurious_calls() {
    let path = rp("deps/rabbit/src/rabbit_definitions.erl");
    let added = "\
-export([fold_queues/3]).
-spec fold_queues(Selector, Acc, Fun) -> Acc when
      Fun :: fun((Queue :: map(), Acc) -> Acc),
      Acc :: term().
fold_queues(Selector, AccIn, Fun) ->
    lists:foldl(fun(Q, Acc0) -> Fun(queue_definition(Q), Acc0) end, AccIn, all_queues()).
";
    let target = "-module(rabbit_definitions).
fold_queues(_, A, _) -> A.
queue_definition(Q) -> Q.
all_queues() -> [].
";
    let reasons = analyse(
        &path,
        added,
        &[("deps/rabbit/src/rabbit_definitions.erl", target)],
    );
    assert!(reasons.is_empty(), "spurious flags: {reasons:?}");
}

// A function defined later in the target file still resolves: the whole
// file is parsed, not just a prefix.
#[test]
fn a_forward_reference_resolves() {
    let path = rp("deps/rabbit/src/r.erl");
    let reasons = analyse(
        &path,
        "g() -> later().\n",
        &[(
            "deps/rabbit/src/r.erl",
            "-module(r).\ng() -> later().\nlater() -> ok.\n",
        )],
    );
    assert!(reasons.is_empty());
}

// Return-shape drift over resolved local calls

const IDX: &str = "deps/rabbit/src/rabbit_classic_queue_index_v2.erl";

fn drifted(reasons: &[Reason]) -> Vec<(String, String, String, u32)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::LocalCallReturnShapeDrift {
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

fn idx_module(spec: &str) -> String {
    format!("-module(rabbit_classic_queue_index_v2).\n{spec}info(S) -> S.\n")
}

fn analyse_shapes(added: &str, target_spec: &str, source_spec: &str) -> LocalCallAnalysis {
    let path = rp(IDX);
    let target_text = idx_module(target_spec);
    let source_text = idx_module(source_spec);
    analyse_full(
        &path,
        added,
        &[(IDX, target_text.as_str())],
        Some(&[(IDX, source_text.as_str())]),
    )
}

#[test]
fn a_drifted_local_return_shape_is_flagged_with_both_signatures() {
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\n",
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
    assert!(analysis.reasons.iter().all(|r| !r.is_blocking()));
}

#[test]
fn the_same_local_shape_modulo_whitespace_is_clean() {
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\n",
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
fn reordered_local_union_members_are_clean() {
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\n",
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
fn a_missing_local_spec_withholds_and_is_counted() {
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\n",
        "",
        "-spec info(state()) -> list().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_no_spec, 1);
}

#[test]
fn an_unmodelled_local_return_type_withholds_and_is_counted() {
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\n",
        "-spec info(state()) -> <<_:8, _:_*8>>.\n",
        "-spec info(state()) -> list().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_unknown_type, 1);
}

#[test]
fn no_source_checkout_withholds_local_calls_once_each() {
    let path = rp(IDX);
    let target_text = idx_module("-spec info(state()) -> binary().\n");
    let analysis = analyse_full(
        &path,
        "publish(S) -> info(S).\nack(S) -> info(S).\n",
        &[(IDX, target_text.as_str())],
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
fn the_same_drifted_local_call_twice_yields_one_reason() {
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\nack(S) -> info(S).\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert_eq!(drifted(&analysis.reasons).len(), 1);
    assert_eq!(analysis.shape_checks.compared, 1);
}

#[test]
fn a_patch_defined_local_callee_gets_no_shape_check() {
    // The added text defines info/1 itself: patch-owned, no comparison.
    let analysis = analyse_shapes(
        "publish(S) -> info(S).\ninfo(S) -> S.\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

#[test]
fn a_patch_rewritten_local_spec_gets_no_shape_check() {
    let analysis = analyse_shapes(
        "-spec info(state()) -> map().\npublish(S) -> info(S).\n",
        "-spec info(state()) -> binary().\n",
        "-spec info(state()) -> list().\n",
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

#[test]
fn an_imported_resolution_is_withheld_and_counted() {
    let path = rp(IDX);
    let target_text =
        "-module(rabbit_classic_queue_index_v2).\n-import(lists, [keyfind/3]).\n".to_owned();
    let analysis = analyse_full(
        &path,
        "publish(S) -> keyfind(a, 1, S).\n",
        &[(IDX, target_text.as_str())],
        Some(&[(IDX, target_text.as_str())]),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks.withheld_imported, 1);
    assert_eq!(analysis.shape_checks.compared, 0);
}

#[test]
fn a_bif_is_never_shape_checked() {
    let path = rp(IDX);
    let target_text = idx_module("-spec info(state()) -> binary().\n");
    let analysis = analyse_full(
        &path,
        "publish(S) -> iolist_to_binary(S).\n",
        &[(IDX, target_text.as_str())],
        Some(&[(IDX, target_text.as_str())]),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.shape_checks, ShapeCheckTally::default());
}

// Calls the patch resolves through its own added -import

const CALLER: &str = "deps/rabbitmq_auth_backend_oauth2/test/unit_SUITE.erl";
const COERCION: &str = "deps/rabbit_common/src/rabbit_data_coercion.erl";

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

// The imported module is absent from the target tree, as an OTP or
// stdlib module would be: the call is neither flagged as a local
// undefined nor as a qualified one.
#[test]
fn a_patch_imported_call_to_an_absent_module_is_not_flagged() {
    let path = rp(CALLER);
    let analysis = analyse_full(
        &path,
        "-import(lists, [foldl/3]).\ng(L) -> foldl(fun(X, A) -> A end, 0, L).\n",
        &[(CALLER, "-module(unit_SUITE).\n")],
        None,
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
}

// The same call without the patch import is a local undefined.
#[test]
fn the_same_call_without_the_patch_import_is_flagged_locally() {
    let path = rp(CALLER);
    let reasons = analyse(
        &path,
        "g(L) -> foldl(fun(X, A) -> A end, 0, L).\n",
        &[(CALLER, "-module(unit_SUITE).\n")],
    );
    assert_eq!(flagged(&reasons), [("foldl", 3)]);
}

// The import sets are arity-exact: importing f/1 does not suppress a
// call to f/2.
#[test]
fn a_patch_import_of_one_arity_does_not_suppress_another() {
    let path = rp(CALLER);
    let reasons = analyse(
        &path,
        "-import(rabbit_data_coercion, [to_binary/1]).\ng(A, B) -> to_binary(A, B).\n",
        &[(CALLER, "-module(unit_SUITE).\n")],
    );
    assert_eq!(flagged(&reasons), [("to_binary", 2)]);
}

// The HF-36 incident: the added text imports nothing for the helper,
// so the target module that neither defines nor imports it is flagged.
#[test]
fn a_call_the_patch_neither_defines_nor_imports_is_flagged_locally() {
    let path = rp(CALLER);
    let reasons = analyse(
        &path,
        "t(C) -> user_login_authentication(rabbit_auth_backend_oauth2, C).\n",
        &[(CALLER, "-module(unit_SUITE).\n")],
    );
    assert_eq!(flagged(&reasons), [("user_login_authentication", 2)]);
}

// The imported module is first-party and lacks the function: the call
// resolves as a qualified reference and is flagged there, not locally.
#[test]
fn a_patch_import_of_a_first_party_absent_function_is_flagged_qualified() {
    let path = rp(CALLER);
    let analysis = analyse_full(
        &path,
        "-import(rabbit_data_coercion, [to_binary/1]).\ng(X) -> to_binary(X).\n",
        &[
            (CALLER, "-module(unit_SUITE).\n"),
            (
                COERCION,
                "-module(rabbit_data_coercion).\n-export([to_atom/1]).\nto_atom(X) -> X.\n",
            ),
        ],
        None,
    );
    assert!(flagged(&analysis.reasons).is_empty());
    assert_eq!(
        qualified_flagged(&analysis.reasons),
        [("rabbit_data_coercion".to_owned(), "to_binary".to_owned(), 1)]
    );
}

// The imported module is first-party and exports the function: clean.
#[test]
fn a_patch_import_of_a_first_party_exported_function_is_clean() {
    let path = rp(CALLER);
    let analysis = analyse_full(
        &path,
        "-import(rabbit_data_coercion, [to_binary/1]).\ng(X) -> to_binary(X).\n",
        &[
            (CALLER, "-module(unit_SUITE).\n"),
            (
                COERCION,
                "-module(rabbit_data_coercion).\n-export([to_binary/1]).\nto_binary(X) -> X.\n",
            ),
        ],
        None,
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
}

// A patch-imported call whose target -spec return shape drifts yields
// the qualified shape-drift reason.
#[test]
fn a_patch_imported_call_with_drifted_target_spec_yields_qualified_drift() {
    let path = rp(CALLER);
    let target_coercion = "-module(rabbit_data_coercion).\n-export([to_binary/1]).\n\
-spec to_binary(term()) -> binary().\nto_binary(X) -> X.\n";
    let source_coercion = "-module(rabbit_data_coercion).\n-export([to_binary/1]).\n\
-spec to_binary(term()) -> list().\nto_binary(X) -> X.\n";
    let analysis = analyse_full(
        &path,
        "-import(rabbit_data_coercion, [to_binary/1]).\ng(X) -> to_binary(X).\n",
        &[
            (CALLER, "-module(unit_SUITE).\n"),
            (COERCION, target_coercion),
        ],
        Some(&[(COERCION, source_coercion)]),
    );
    let drift: Vec<_> = analysis
        .reasons
        .iter()
        .filter(|r| matches!(r, Reason::QualifiedCallReturnShapeDrift { .. }))
        .collect();
    assert_eq!(drift.len(), 1, "reasons: {:?}", analysis.reasons);
}

// The patch adds both the import and the callee's definition and
// -export in the module's own file: patch_provided suppresses it.
#[test]
fn a_patch_that_adds_the_import_and_the_definition_is_not_flagged() {
    let path = rp(CALLER);
    let caller_added = "-import(rabbit_data_coercion, [to_binary/1]).\ng(X) -> to_binary(X).\n";
    let patch_added = patch_provided(&[
        (module("unit_SUITE"), caller_added),
        (
            module("rabbit_data_coercion"),
            "-export([to_binary/1]).\nto_binary(X) -> X.\n",
        ),
    ]);
    let analysis = analyse_ctx(
        &path,
        caller_added,
        &[],
        &patch_added,
        &[
            (CALLER, "-module(unit_SUITE).\n"),
            (
                COERCION,
                "-module(rabbit_data_coercion).\n-export([to_atom/1]).\nto_atom(X) -> X.\n",
            ),
        ],
        None,
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
}

// A tracked-dependency import defers to its snapshot: the module is in
// covered_modules, so the live tree is not consulted and nothing flags.
#[test]
fn a_patch_import_of_a_covered_module_is_withheld() {
    let path = rp(CALLER);
    let analysis = analyse_ctx(
        &path,
        "-import(ra, [members/1]).\ng(S) -> members(S).\n",
        &["ra"],
        &PatchProvided::default(),
        &[
            (CALLER, "-module(unit_SUITE).\n"),
            (
                "deps/ra/src/ra.erl",
                "-module(ra).\n-export([overview/1]).\noverview(X) -> X.\n",
            ),
        ],
        None,
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
}
