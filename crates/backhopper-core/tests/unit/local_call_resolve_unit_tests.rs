// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_local_calls` against an injected target-module reader.

use std::collections::BTreeMap;

use backhopper_core::compat::local_call_resolve::{LocalCallSubject, analyse_local_calls};
use backhopper_core::model::names::RelativePath;
use backhopper_core::model::verdict::Reason;

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

fn analyse(path: &RelativePath, added: &str, target: &[(&str, &str)]) -> Vec<Reason> {
    let subjects = [LocalCallSubject {
        source_path: path,
        added_text: added,
    }];
    analyse_local_calls(&subjects, &reader(target))
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
