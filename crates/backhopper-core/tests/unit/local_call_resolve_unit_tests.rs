// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_local_calls` against an injected target-module reader.

use std::collections::BTreeMap;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::local_call_resolve::{LocalCallAnalysis, analyse_local_calls};
use backhopper_core::model::names::RelativePath;
use backhopper_core::model::verdict::{Reason, ShapeCheckTally};

fn reader(files: &[(&str, &str)]) -> impl Fn(&RelativePath) -> Option<String> {
    let map: BTreeMap<String, String> = files
        .iter()
        .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
        .collect();
    move |path: &RelativePath| map.get(path.as_str()).cloned()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
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

fn analyse_full(
    path: &RelativePath,
    added: &str,
    target: &[(&str, &str)],
    source: Option<&[(&str, &str)]>,
) -> LocalCallAnalysis {
    let subjects = [AddedLinesSubject {
        source_path: path,
        added_text: added,
        line_map: &[],
    }];
    let read_target = reader(target);
    let read_source_fn = source.map(reader);
    let read_source = read_source_fn
        .as_ref()
        .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
    analyse_local_calls(&subjects, &read_target, read_source)
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

// Return-shape drift over resolved local calls (046 §B)

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
