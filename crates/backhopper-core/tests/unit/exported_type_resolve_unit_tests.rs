// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_exported_types` against an in-memory target tree. The
//! incident these model: `v4.x` renamed `rabbit_proxy_socket/0` to
//! `proxy_socket/0`, the rename never cascaded, and a pick exported a
//! type `v3.13.x` does not declare.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::exported_type_resolve::analyse_exported_types;
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{CommitSha, GitRef, RelativePath};
use backhopper_core::model::verdict::Reason;

const ERL: &str = "deps/rabbit_common/src/rabbit_net.erl";

const TARGET_BEFORE_RENAME: &str = "-module(rabbit_net).\n\
     -type socket() :: inet:socket().\n\
     -type rabbit_proxy_socket() :: {rabbit_proxy_socket, inet:socket(), term()}.\n";

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

/// Added lines start at file line 21: a mapped line proves the map was applied.
fn analyse(added: &str, files: &[(&str, &str)]) -> Vec<Reason> {
    let path = rp(ERL);
    let line_map: Vec<u32> = (1..=added.lines().count() as u32).map(|l| l + 20).collect();
    let subjects = vec![AddedLinesSubject {
        source_path: &path,
        added_text: added,
        line_map: &line_map,
    }];
    let paths: Vec<&str> = files.iter().map(|(p, _)| *p).collect();
    analyse_exported_types(&subjects, &BTreeMap::new(), &index(&paths), &reader(files))
}

fn on_erl(added: &str, target: &str) -> Vec<Reason> {
    analyse(added, &[(ERL, target)])
}

fn flagged(reasons: &[Reason]) -> Vec<(String, u8)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::ExportedTypeUndefinedOnTarget {
                type_name, arity, ..
            } => Some((type_name.as_str().to_owned(), arity.get())),
            _ => None,
        })
        .collect()
}

#[test]
fn an_exported_type_the_target_does_not_define_is_flagged() {
    let reasons = on_erl(
        "-export_type([socket/0, proxy_socket/0]).\n",
        TARGET_BEFORE_RENAME,
    );
    assert_eq!(flagged(&reasons), vec![("proxy_socket".to_owned(), 0)]);
}

#[test]
fn a_flagged_entry_reports_the_file_line_not_the_blob_line() {
    let reasons = on_erl("-export_type([proxy_socket/0]).\n", TARGET_BEFORE_RENAME);
    match &reasons[0] {
        Reason::ExportedTypeUndefinedOnTarget {
            source_path, line, ..
        } => {
            assert_eq!(source_path, &rp(ERL));
            assert_eq!(*line, 21);
        }
        other => panic!("unexpected reason: {other:?}"),
    }
}

#[test]
fn an_exported_type_the_target_defines_is_clean() {
    assert!(on_erl("-export_type([socket/0]).\n", TARGET_BEFORE_RENAME).is_empty());
}

#[test]
fn an_exported_type_the_patch_itself_defines_is_clean() {
    let added = "-export_type([proxy_socket/0]).\n\
                 -type proxy_socket() :: {rabbit_proxy_socket, inet:socket(), term()}.\n";
    assert!(on_erl(added, TARGET_BEFORE_RENAME).is_empty());
}

#[test]
fn a_type_declared_in_an_included_header_is_not_flagged() {
    // rabbit_fifo exports ten types declared in rabbit_fifo.hrl
    let erl = "-module(rabbit_fifo).\n-include(\"rabbit_fifo.hrl\").\n";
    let hrl = "-type msg_id() :: non_neg_integer().\n-type consumer_tag() :: binary().\n";
    let reasons = analyse(
        "-export_type([msg_id/0, consumer_tag/0]).\n",
        &[(ERL, erl), ("deps/rabbit_common/src/rabbit_fifo.hrl", hrl)],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

#[test]
fn a_type_declared_in_a_transitively_included_header_is_not_flagged() {
    let erl = "-module(rabbit_fifo).\n-include(\"outer.hrl\").\n";
    let outer = "-include(\"inner.hrl\").\n";
    let inner = "-type name() :: binary().\n";
    let reasons = analyse(
        "-export_type([name/0]).\n",
        &[
            (ERL, erl),
            ("deps/rabbit_common/src/outer.hrl", outer),
            ("deps/rabbit_common/src/inner.hrl", inner),
        ],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

#[test]
fn a_macro_expanded_attribute_form_withholds() {
    // rabbit_framing declares its whole type surface this way
    let target = "-module(rabbit_framing).\n\
                  -define(protocol_type(T), type(T :: rabbit_framing_amqp_0_8:T)).\n\
                  -?protocol_type(amqp_table()).\n";
    assert!(on_erl("-export_type([amqp_table/0]).\n", target).is_empty());
}

#[test]
fn a_macro_expanded_attribute_in_a_header_withholds() {
    let erl = "-module(rabbit_framing).\n-include(\"framing.hrl\").\n";
    let hrl = "-?protocol_type(amqp_table()).\n";
    let reasons = analyse(
        "-export_type([amqp_table/0]).\n",
        &[(ERL, erl), ("deps/rabbit_common/src/framing.hrl", hrl)],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

#[test]
fn an_unreadable_header_withholds() {
    // the missing header could hold the declaration
    let erl = "-module(rabbit_fifo).\n-include(\"missing.hrl\").\n";
    assert!(analyse("-export_type([name/0]).\n", &[(ERL, erl)]).is_empty());
}

#[test]
fn an_unreadable_target_file_withholds() {
    assert!(analyse("-export_type([proxy_socket/0]).\n", &[]).is_empty());
}

#[test]
fn a_target_parse_transform_withholds() {
    let target = "-module(rabbit_net).\n-compile({parse_transform, lager_transform}).\n";
    assert!(on_erl("-export_type([proxy_socket/0]).\n", target).is_empty());
}

#[test]
fn an_incomplete_export_type_list_withholds() {
    let added = "-export_type([proxy_socket/0, ?EXTRA_TYPES]).\n";
    assert!(on_erl(added, TARGET_BEFORE_RENAME).is_empty());
}

#[test]
fn a_target_export_all_does_not_withhold() {
    // export_all hides functions, not types: the axis still resolves
    let target = "-module(rabbit_net).\n\
                  -compile(export_all).\n\
                  -type socket() :: inet:socket().\n";
    let reasons = on_erl("-export_type([proxy_socket/0]).\n", target);
    assert_eq!(flagged(&reasons), vec![("proxy_socket".to_owned(), 0)]);
}

#[test]
fn one_reason_per_type_and_arity() {
    let added = "-export_type([proxy_socket/0]).\n-export_type([proxy_socket/0]).\n";
    assert_eq!(on_erl(added, TARGET_BEFORE_RENAME).len(), 1);
}

#[test]
fn arity_is_part_of_the_key() {
    let target = "-module(rabbit_net).\n-type result(Ok, Err) :: {ok, Ok} | {error, Err}.\n";
    let reasons = on_erl("-export_type([result/2, result/0]).\n", target);
    assert_eq!(flagged(&reasons), vec![("result".to_owned(), 0)]);
}

// The include line exists only in the patch; the header on target declares the type.
#[test]
fn a_patch_added_include_line_supplying_the_exported_type_is_clean() {
    let erl = "-module(rabbit_net).\n";
    let hrl = "-type name() :: binary().\n";
    let reasons = analyse(
        "-include(\"types.hrl\").\n-export_type([name/0]).\n",
        &[(ERL, erl), ("deps/rabbit_common/src/types.hrl", hrl)],
    );
    assert!(reasons.is_empty(), "{reasons:?}");
}

// The axis resolves against the target module's text, so a file the target lacks stays out of scope.
#[test]
fn an_added_file_is_still_skipped() {
    let added = "-module(rabbit_net).\n-export_type([nowhere_defined/0]).\n";
    assert!(analyse(added, &[]).is_empty());
}

#[test]
fn a_patch_that_adds_no_export_type_yields_nothing() {
    let added = "-spec peername(socket()) -> {ok, term()}.\n";
    assert!(on_erl(added, TARGET_BEFORE_RENAME).is_empty());
}
