// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_define_symbols` against an in-memory target tree: the
//! reader is injected, so no git fixture is needed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backhopper_core::compat::define_resolve::{DefineSubject, analyse_define_symbols};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{CommitSha, GitRef, RelativePath};
use backhopper_core::model::verdict::Reason;

fn index(paths: &[&str]) -> TargetTreeIndex {
    let present: BTreeSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
    TargetTreeIndex::from_parts(
        PathBuf::from("/repo"),
        GitRef::new("HEAD").unwrap(),
        CommitSha::new("a".repeat(40)).unwrap(),
        present,
    )
}

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

fn macros_flagged(reasons: &[Reason]) -> Vec<&str> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::MacroUndefinedOnTarget { macro_name, .. } => Some(macro_name.as_str()),
            _ => None,
        })
        .collect()
}

fn records_flagged(reasons: &[Reason]) -> Vec<&str> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::RecordUndefinedOnTarget { record_name, .. } => Some(record_name.as_str()),
            _ => None,
        })
        .collect()
}

fn analyse(
    path: &RelativePath,
    added: &str,
    idx: &TargetTreeIndex,
    files: &[(&str, &str)],
) -> Vec<Reason> {
    let subjects = [DefineSubject {
        source_path: path,
        added_text: added,
        line_map: &[],
    }];
    analyse_define_symbols(&subjects, idx, &reader(files))
}

// A file present on target but missing the define the added line uses.
#[test]
fn macro_undefined_on_target_is_flagged() {
    let path = rp("deps/rabbit/src/rabbit_mgmt_util.erl");
    let reasons = analyse(
        &path,
        "cookie_path() -> ?OAUTH2_BOOTSTRAP_PATH.\n",
        &index(&["deps/rabbit/src/rabbit_mgmt_util.erl"]),
        &[(
            "deps/rabbit/src/rabbit_mgmt_util.erl",
            "-module(rabbit_mgmt_util).\n",
        )],
    );
    assert_eq!(macros_flagged(&reasons), ["OAUTH2_BOOTSTRAP_PATH"]);
}

#[test]
fn macro_defined_in_the_target_file_is_clean() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> ?FOO.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[("deps/rabbit/src/x.erl", "-define(FOO, 1).\n")],
    );
    assert!(reasons.is_empty());
}

// The define lives in an included header: the resolver follows the
// `-include` to find it.
#[test]
fn macro_defined_via_an_included_header_is_clean() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> ?FOO.\n",
        &index(&["deps/rabbit/src/x.erl", "deps/rabbit/src/defs.hrl"]),
        &[
            (
                "deps/rabbit/src/x.erl",
                "-module(x).\n-include(\"defs.hrl\").\n",
            ),
            ("deps/rabbit/src/defs.hrl", "-define(FOO, 1).\n"),
        ],
    );
    assert!(reasons.is_empty());
}

// An include the target tree does not ship leaves the define set
// incomplete, so the symbol is suppressed rather than false-flagged.
#[test]
fn an_unresolved_include_suppresses_flagging() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> ?FOO.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[(
            "deps/rabbit/src/x.erl",
            "-module(x).\n-include(\"absent.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn a_stdlib_include_lib_suppresses_macro_flagging() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> ?FOO.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[(
            "deps/rabbit/src/x.erl",
            "-module(x).\n-include_lib(\"kernel/include/logger.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

// The eunit assert family lives in the skipped eunit.hrl.
#[test]
fn an_eunit_assert_macro_is_not_flagged() {
    let path = rp("deps/rabbit/test/x_SUITE.erl");
    let reasons = analyse(
        &path,
        "t() -> ?assertEqual(1, 1).\n",
        &index(&["deps/rabbit/test/x_SUITE.erl"]),
        &[(
            "deps/rabbit/test/x_SUITE.erl",
            "-module(x_SUITE).\n-include_lib(\"eunit/include/eunit.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

// Records use the same completeness flag as macros.
#[test]
fn a_stdlib_include_lib_suppresses_record_flagging() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f(S) -> S#state.field.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[(
            "deps/rabbit/src/x.erl",
            "-module(x).\n-include_lib(\"kernel/include/file.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn macro_defined_by_the_patch_is_clean() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "-define(FOO, 1).\nf() -> ?FOO.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[("deps/rabbit/src/x.erl", "-module(x).\n")],
    );
    assert!(reasons.is_empty());
}

#[test]
fn predefined_macros_are_never_flagged() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> ?MODULE:g(?LINE).\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[("deps/rabbit/src/x.erl", "-module(x).\n")],
    );
    assert!(reasons.is_empty());
}

#[test]
fn a_record_undefined_on_target_is_flagged() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f(S) -> S#state.field.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[("deps/rabbit/src/x.erl", "-module(x).\n")],
    );
    assert_eq!(records_flagged(&reasons), ["state"]);
}

#[test]
fn a_record_defined_on_target_is_clean() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> #state{}.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[("deps/rabbit/src/x.erl", "-record(state, {field}).\n")],
    );
    assert!(reasons.is_empty());
}

// `#{...}` is a map, not a record use: never flagged.
#[test]
fn a_map_literal_is_not_a_record_use() {
    let path = rp("deps/rabbit/src/x.erl");
    let reasons = analyse(
        &path,
        "f() -> #{key => value}.\n",
        &index(&["deps/rabbit/src/x.erl"]),
        &[("deps/rabbit/src/x.erl", "-module(x).\n")],
    );
    assert!(reasons.is_empty());
}
