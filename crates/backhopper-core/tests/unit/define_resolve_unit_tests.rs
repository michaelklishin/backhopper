// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_define_symbols` against an in-memory target tree: the
//! reader is injected, so no git fixture is needed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::define_resolve::analyse_define_symbols;
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
    analyse_with_patch_added(path, added, &[], idx, files)
}

fn analyse_with_patch_added(
    path: &RelativePath,
    added: &str,
    patch_added: &[(&str, &str)],
    idx: &TargetTreeIndex,
    files: &[(&str, &str)],
) -> Vec<Reason> {
    let subjects = [AddedLinesSubject {
        source_path: path,
        added_text: added,
        line_map: &[],
    }];
    let patch_added: BTreeMap<RelativePath, String> = patch_added
        .iter()
        .map(|(p, c)| (RelativePath::new(*p).unwrap(), (*c).to_owned()))
        .collect();
    analyse_define_symbols(&subjects, &patch_added, idx, &reader(files))
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
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> ?DEFAULT_TIMEOUT.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-define(DEFAULT_TIMEOUT, 1).\n",
        )],
    );
    assert!(reasons.is_empty());
}

// The define is in an included header: the resolver follows the -include.
#[test]
fn macro_defined_via_an_included_header_is_clean() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> ?DEFAULT_TIMEOUT.\n",
        &index(&[
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "deps/rabbit/src/defs.hrl",
        ]),
        &[
            (
                "deps/rabbit/src/rabbit_amqqueue.erl",
                "-module(rabbit_amqqueue).\n-include(\"defs.hrl\").\n",
            ),
            ("deps/rabbit/src/defs.hrl", "-define(DEFAULT_TIMEOUT, 1).\n"),
        ],
    );
    assert!(reasons.is_empty());
}

// An include the target tree lacks leaves the define set incomplete: suppress rather than false-flag.
#[test]
fn an_unresolved_include_suppresses_flagging() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> ?DEFAULT_TIMEOUT.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n-include(\"absent.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn a_stdlib_include_lib_suppresses_macro_flagging() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> ?DEFAULT_TIMEOUT.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n-include_lib(\"kernel/include/logger.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

// The eunit assert family is defined in the skipped eunit.hrl.
#[test]
fn an_eunit_assert_macro_is_not_flagged() {
    let path = rp("deps/rabbit/test/rabbit_amqqueue_SUITE.erl");
    let reasons = analyse(
        &path,
        "verify() -> ?assertEqual(1, 1).\n",
        &index(&["deps/rabbit/test/rabbit_amqqueue_SUITE.erl"]),
        &[(
            "deps/rabbit/test/rabbit_amqqueue_SUITE.erl",
            "-module(rabbit_amqqueue_SUITE).\n-include_lib(\"eunit/include/eunit.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

// Records use the same completeness flag as macros.
#[test]
fn a_stdlib_include_lib_suppresses_record_flagging() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init(S) -> S#state.field.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n-include_lib(\"kernel/include/file.hrl\").\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn macro_defined_by_the_patch_is_clean() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "-define(DEFAULT_TIMEOUT, 1).\ninit() -> ?DEFAULT_TIMEOUT.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn predefined_macros_are_never_flagged() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> ?MODULE:members(?LINE).\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n",
        )],
    );
    assert!(reasons.is_empty());
}

#[test]
fn a_record_undefined_on_target_is_flagged() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init(S) -> S#state.field.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n",
        )],
    );
    assert_eq!(records_flagged(&reasons), ["state"]);
}

#[test]
fn a_record_defined_on_target_is_clean() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> #state{}.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-record(state, {field}).\n",
        )],
    );
    assert!(reasons.is_empty());
}

// The added file is absent on target, so its includes come from the added text alone.
#[test]
fn an_added_file_whose_macro_comes_from_a_target_resolvable_include_is_clean() {
    let path = rp("deps/rabbit/src/rabbit_new.erl");
    let reasons = analyse(
        &path,
        "-module(rabbit_new).\n-include(\"defs.hrl\").\ninit() -> ?DEFAULT_TIMEOUT.\n",
        &index(&["deps/rabbit/src/defs.hrl"]),
        &[("deps/rabbit/src/defs.hrl", "-define(DEFAULT_TIMEOUT, 1).\n")],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

// The proper dep is fetched, not committed, so the include cannot resolve: withhold instead of flagging.
#[test]
fn an_added_file_with_an_unresolvable_non_stdlib_include_lib_is_withheld() {
    let path = rp("deps/rabbitmq_management/test/mgmt_prop_SUITE.erl");
    let reasons = analyse(
        &path,
        "-module(mgmt_prop_SUITE).\n\
         -include_lib(\"proper/include/proper.hrl\").\n\
         prop_roundtrip() -> ?FORALL(X, list(atom()), X =:= X).\n\
         shape(S) -> S#forall_props.field.\n",
        &index(&[]),
        &[],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

#[test]
fn an_added_file_with_a_stdlib_include_lib_is_withheld() {
    let path = rp("deps/rabbit/test/rabbit_new_SUITE.erl");
    let reasons = analyse(
        &path,
        "-module(rabbit_new_SUITE).\n\
         -include_lib(\"eunit/include/eunit.hrl\").\n\
         verify() -> ?assertEqual(1, 1), #file_info{}.\n",
        &index(&[]),
        &[],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

// All includes resolve, so the closure is fully known and the miss is real.
#[test]
fn an_added_file_whose_macro_resolves_nowhere_is_still_flagged() {
    let path = rp("deps/rabbit/src/rabbit_new.erl");
    let reasons = analyse(
        &path,
        "-module(rabbit_new).\n-include(\"defs.hrl\").\ninit() -> ?TRULY_ABSENT.\n",
        &index(&["deps/rabbit/src/defs.hrl"]),
        &[("deps/rabbit/src/defs.hrl", "-define(DEFAULT_TIMEOUT, 1).\n")],
    );
    assert_eq!(macros_flagged(&reasons), ["TRULY_ABSENT"]);
}

// The include line exists only in the patch, but the header it names is on target.
#[test]
fn a_patch_added_include_line_on_a_modified_file_supplies_its_macros() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "-include(\"defs.hrl\").\ninit() -> ?DEFAULT_TIMEOUT.\n",
        &index(&[
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "deps/rabbit/src/defs.hrl",
        ]),
        &[
            (
                "deps/rabbit/src/rabbit_amqqueue.erl",
                "-module(rabbit_amqqueue).\n",
            ),
            ("deps/rabbit/src/defs.hrl", "-define(DEFAULT_TIMEOUT, 1).\n"),
        ],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

// A patch that adds a module and its header together stays fully analyzable.
#[test]
fn a_patch_added_header_supplies_the_added_module() {
    let path = rp("deps/rabbit/src/rabbit_new.erl");
    let reasons = analyse_with_patch_added(
        &path,
        "-module(rabbit_new).\n-include(\"rabbit_new.hrl\").\n\
         init() -> {?NEW_TIMEOUT, #new_state{}}.\n",
        &[(
            "deps/rabbit/src/rabbit_new.hrl",
            "-define(NEW_TIMEOUT, 5).\n-record(new_state, {field}).\n",
        )],
        &index(&[]),
        &[],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

// A patch-added header walks at depth like any other: its own includes resolve against the map too.
#[test]
fn a_patch_added_header_chain_resolves_transitively() {
    let path = rp("deps/rabbit/src/rabbit_new.erl");
    let reasons = analyse_with_patch_added(
        &path,
        "-module(rabbit_new).\n-include(\"outer.hrl\").\ninit() -> ?INNER_TIMEOUT.\n",
        &[
            ("deps/rabbit/src/outer.hrl", "-include(\"inner.hrl\").\n"),
            ("deps/rabbit/src/inner.hrl", "-define(INNER_TIMEOUT, 5).\n"),
        ],
        &index(&[]),
        &[],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

// The pair resolves fully, so the axis keeps its sensitivity: no suppression.
#[test]
fn a_patch_added_header_pair_still_flags_a_macro_defined_nowhere() {
    let path = rp("deps/rabbit/src/rabbit_new.erl");
    let reasons = analyse_with_patch_added(
        &path,
        "-module(rabbit_new).\n-include(\"rabbit_new.hrl\").\ninit() -> ?TRULY_ABSENT.\n",
        &[(
            "deps/rabbit/src/rabbit_new.hrl",
            "-define(NEW_TIMEOUT, 5).\n",
        )],
        &index(&[]),
        &[],
    );
    assert_eq!(macros_flagged(&reasons), ["TRULY_ABSENT"]);
}

// A path present on both sides reads the target text: the pick's collision is the apply axis's problem.
#[test]
fn a_patch_added_header_that_also_exists_on_target_reads_the_target_text() {
    let path = rp("deps/rabbit/src/rabbit_new.erl");
    let reasons = analyse_with_patch_added(
        &path,
        "-module(rabbit_new).\n-include(\"shared.hrl\").\ninit() -> ?PATCH_ONLY.\n",
        &[("deps/rabbit/src/shared.hrl", "-define(PATCH_ONLY, 1).\n")],
        &index(&["deps/rabbit/src/shared.hrl"]),
        &[("deps/rabbit/src/shared.hrl", "-define(TARGET_ONLY, 1).\n")],
    );
    assert_eq!(macros_flagged(&reasons), ["PATCH_ONLY"]);
}

// #{...} is a map, not a record use: never flagged.
#[test]
fn a_map_literal_is_not_a_record_use() {
    let path = rp("deps/rabbit/src/rabbit_amqqueue.erl");
    let reasons = analyse(
        &path,
        "init() -> #{key => value}.\n",
        &index(&["deps/rabbit/src/rabbit_amqqueue.erl"]),
        &[(
            "deps/rabbit/src/rabbit_amqqueue.erl",
            "-module(rabbit_amqqueue).\n",
        )],
    );
    assert!(reasons.is_empty());
}
