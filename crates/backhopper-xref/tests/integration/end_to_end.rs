// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::{ApplicationName, BehaviourName, ModuleName};
use backhopper_xref::{XrefBuilder, diff_xrefs, is_suite_module, suites_referencing};

fn rabbitmq_like(modules: &[(&str, &str)]) -> backhopper_xref::Xref<backhopper_xref::Functions> {
    use backhopper_xref::ProjectLayout;
    let mut b = XrefBuilder::new().with_layout(ProjectLayout::rabbitmq_main());
    let app = ApplicationName::new("rabbit".to_owned()).unwrap();
    let files: Vec<(PathBuf, Vec<u8>)> = modules
        .iter()
        .map(|(name, body)| {
            let p = PathBuf::from(format!("deps/rabbit/src/{}.erl", name));
            (p, body.as_bytes().to_vec())
        })
        .collect();
    b.add_application(app, files).unwrap();
    b.build().unwrap()
}

#[test]
fn end_to_end_rabbitmq_like_tree_yields_expected_module_set() {
    let x = rabbitmq_like(&[
        (
            "rabbit_db",
            "-module(rabbit_db).\n-export([read/1]).\nread(_) -> ok.\n",
        ),
        (
            "rabbit_vhost",
            "-module(rabbit_vhost).\n-export([list/0]).\nlist() -> rabbit_db:read(vhosts).\n",
        ),
    ]);
    assert_eq!(x.graph().module_count(), 2);
    let callers = x.module_called_by(&ModuleName::new("rabbit_db".to_owned()).unwrap());
    assert!(callers.entries.iter().any(|m| m.as_str() == "rabbit_vhost"));
}

#[test]
fn end_to_end_diff_detects_added_call() {
    let from = rabbitmq_like(&[
        (
            "rabbit_db",
            "-module(rabbit_db).\n-export([read/1]).\nread(_) -> ok.\n",
        ),
        (
            "rabbit_vhost",
            "-module(rabbit_vhost).\n-export([list/0]).\nlist() -> ok.\n",
        ),
    ]);
    let to = rabbitmq_like(&[
        (
            "rabbit_db",
            "-module(rabbit_db).\n-export([read/1]).\nread(_) -> ok.\n",
        ),
        (
            "rabbit_vhost",
            "-module(rabbit_vhost).\n-export([list/0]).\nlist() -> rabbit_db:read(vhosts).\n",
        ),
    ]);
    let d = diff_xrefs(&from, &to);
    assert_eq!(d.added_calls.len(), 1);
    assert!(d.removed_calls.is_empty());
}

#[test]
fn end_to_end_suite_selection_returns_dependent_suite() {
    let mut b = XrefBuilder::new().with_layout(backhopper_xref::ProjectLayout::rabbitmq_main());
    let app = ApplicationName::new("rabbit".to_owned()).unwrap();
    b.add_application(
        app,
        vec![
            (
                PathBuf::from("deps/rabbit/src/rabbit_db.erl"),
                b"-module(rabbit_db).\n-export([read/1]).\nread(_) -> ok.\n".to_vec(),
            ),
            (
                PathBuf::from("deps/rabbit/test/db_SUITE.erl"),
                b"-module(db_SUITE).\n-export([t/0]).\nt() -> rabbit_db:read(x).\n".to_vec(),
            ),
            (
                PathBuf::from("deps/rabbit/test/unrelated_SUITE.erl"),
                b"-module(unrelated_SUITE).\n-export([t/0]).\nt() -> ok.\n".to_vec(),
            ),
        ],
    )
    .unwrap();
    let x = b.build().unwrap();
    let modified: BTreeSet<ModuleName> =
        std::iter::once(ModuleName::new("rabbit_db".to_owned()).unwrap()).collect();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    let names: Vec<&str> = suites.iter().map(|s| s.module.as_str()).collect();
    assert_eq!(names, vec!["db_SUITE"]);
}

#[test]
fn end_to_end_behaviour_implementer_appears_in_query() {
    let x = rabbitmq_like(&[(
        "rabbit_handler",
        "-module(rabbit_handler).\n-behaviour(gen_server).\n-export([handle_call/3]).\nhandle_call(_, _, S) -> {reply, ok, S}.\n",
    )]);
    let beh = BehaviourName::new("gen_server".to_owned()).unwrap();
    let impls = x.implementers_of(&beh);
    assert!(impls.iter().any(|m| m.as_str() == "rabbit_handler"));
}
