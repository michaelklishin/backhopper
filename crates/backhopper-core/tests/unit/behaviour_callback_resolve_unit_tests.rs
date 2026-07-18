// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_behaviour_callbacks`: a declared behaviour's `-callback`
//! surface compared between the two trees.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::behaviour_callback_resolve::analyse_behaviour_callbacks;
use backhopper_core::model::names::{ModuleName, RelativePath};
use backhopper_core::model::verdict::Reason;

const BQ_PATH: &str = "deps/rabbit/src/rabbit_backing_queue.erl";
const IMPL: &str = "deps/rabbit/src/rabbit_priority_queue.erl";

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn analyse(
    added: &str,
    covered: &[&str],
    target: &[(&str, &str, &str)],
    source: Option<&[(&str, &str)]>,
) -> Vec<Reason> {
    let path = rp(IMPL);
    let subjects = [AddedLinesSubject {
        source_path: &path,
        added_text: added,
        line_map: &[],
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
    analyse_behaviour_callbacks(&subjects, &covered, &resolve, &read, read_source)
}

fn added_on_target(reasons: &[Reason]) -> Vec<(String, u8)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::BehaviourCallbackAddedOnTarget {
                callback, arity, ..
            } => Some((callback.to_string(), arity.get())),
            _ => None,
        })
        .collect()
}

fn drift(reasons: &[Reason]) -> Vec<(String, String, String)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::BehaviourCallbackDriftOnTarget {
                callback,
                source_signature,
                target_signature,
                ..
            } => Some((
                callback.to_string(),
                source_signature.clone(),
                target_signature.clone(),
            )),
            _ => None,
        })
        .collect()
}

const DECLARES: &str = "-module(rabbit_priority_queue).\n-behaviour(rabbit_backing_queue).\n";

fn bq(callbacks: &str) -> String {
    format!("-module(rabbit_backing_queue).\n{callbacks}")
}

#[test]
fn a_target_only_required_callback_is_flagged() {
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n\
         -callback handle_info(term(), state()) -> state().\n");
    let source_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, source_text.as_str())]),
    );
    assert_eq!(added_on_target(&reasons), [("handle_info".to_owned(), 2)]);
    assert!(reasons.iter().all(|r| !r.is_blocking()));
}

#[test]
fn a_target_only_optional_callback_is_silent() {
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n\
         -callback handle_info(term(), state()) -> state().\n\
         -optional_callbacks([handle_info/2]).\n");
    let source_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, source_text.as_str())]),
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_changed_argument_type_is_drift_with_both_signatures() {
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let source_text = bq("-callback purge(queue_state()) -> {count(), state()}.\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, source_text.as_str())]),
    );
    assert_eq!(
        drift(&reasons),
        [(
            "purge".to_owned(),
            "purge(queue_state()) -> {count(), state()}".to_owned(),
            "purge(state()) -> {count(), state()}".to_owned(),
        )]
    );
}

// A same-named callback at a different arity keys separately: added-on-target, not drift.
#[test]
fn an_arity_change_is_added_on_target_not_drift() {
    let target_text = bq("-callback publish(msg(), props(), state()) -> state().\n");
    let source_text = bq("-callback publish(msg(), state()) -> state().\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, source_text.as_str())]),
    );
    assert_eq!(added_on_target(&reasons), [("publish".to_owned(), 3)]);
    assert!(drift(&reasons).is_empty());
}

#[test]
fn an_edit_without_the_behaviour_line_in_added_text_is_silent() {
    // The behaviour declaration entering the patch is the trigger.
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let reasons = analyse(
        "handle(S) -> S.\n",
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, "-module(rabbit_backing_queue).\n")]),
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_whitespace_only_difference_is_clean() {
    let target_text = bq("-callback purge(state()) ->\n    {count(), state()}.\n");
    let source_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, source_text.as_str())]),
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_snapshot_covered_behaviour_is_silent() {
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let reasons = analyse(
        DECLARES,
        &["rabbit_backing_queue"],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, "-module(rabbit_backing_queue).\n")]),
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_behaviour_absent_from_the_target_tree_is_silent() {
    // Existence is BehaviourModuleMissing's finding, not this check's.
    let reasons = analyse(DECLARES, &[], &[], Some(&[]));
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_source_only_callback_is_silent() {
    // The implementation provides it; the target never calls it.
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let source_text = bq("-callback purge(state()) -> {count(), state()}.\n\
         -callback handle_info(term(), state()) -> state().\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        Some(&[(BQ_PATH, source_text.as_str())]),
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn a_legacy_behaviour_without_callback_forms_is_silent() {
    let legacy = "-module(rabbit_backing_queue).\nbehaviour_info(callbacks) -> [{purge, 1}].\n";
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, legacy)],
        Some(&[(BQ_PATH, legacy)]),
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}

#[test]
fn no_source_checkout_is_silent() {
    let target_text = bq("-callback purge(state()) -> {count(), state()}.\n");
    let reasons = analyse(
        DECLARES,
        &[],
        &[("rabbit_backing_queue", BQ_PATH, target_text.as_str())],
        None,
    );
    assert!(reasons.is_empty(), "unexpected: {reasons:?}");
}
