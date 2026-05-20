// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::{ApplicationName, ModuleName};
use backhopper_xref::{XrefBuilder, is_suite_module, suites_referencing};

fn build(sources: &[(&str, &str)]) -> backhopper_xref::Xref<backhopper_xref::Functions> {
    let mut b = XrefBuilder::new();
    let app = ApplicationName::new("t".to_owned()).unwrap();
    let files: Vec<(PathBuf, Vec<u8>)> = sources
        .iter()
        .map(|(name, body)| (PathBuf::from(*name), body.as_bytes().to_vec()))
        .collect();
    b.add_application(app, files).unwrap();
    b.build().unwrap()
}

#[test]
fn suite_predicate_matches_underscore_suite_modules() {
    assert!(is_suite_module(
        &ModuleName::new("vhost_SUITE".to_owned()).unwrap()
    ));
    assert!(!is_suite_module(
        &ModuleName::new("vhost".to_owned()).unwrap()
    ));
}

#[test]
fn suites_referencing_returns_suite_that_calls_modified_module() {
    let x = build(&[
        (
            "rabbit_db_vhost.erl",
            "-module(rabbit_db_vhost).\n-export([get/1]).\nget(_) -> ok.\n",
        ),
        (
            "vhost_SUITE.erl",
            "-module(vhost_SUITE).\n-export([t/0]).\nt() -> rabbit_db_vhost:get(x).\n",
        ),
    ]);
    let modified: BTreeSet<ModuleName> =
        std::iter::once(ModuleName::new("rabbit_db_vhost".to_owned()).unwrap()).collect();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    assert_eq!(suites.len(), 1);
    assert_eq!(suites.iter().next().unwrap().module.as_str(), "vhost_SUITE");
}

#[test]
fn suites_referencing_filters_non_suite_callers() {
    let x = build(&[
        ("lib.erl", "-module(lib).\n-export([f/0]).\nf() -> ok.\n"),
        (
            "consumer.erl",
            "-module(consumer).\n-export([go/0]).\ngo() -> lib:f().\n",
        ),
        (
            "lib_SUITE.erl",
            "-module(lib_SUITE).\n-export([t/0]).\nt() -> lib:f().\n",
        ),
    ]);
    let modified: BTreeSet<ModuleName> =
        std::iter::once(ModuleName::new("lib".to_owned()).unwrap()).collect();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    let names: Vec<&str> = suites.iter().map(|s| s.module.as_str()).collect();
    assert_eq!(names, vec!["lib_SUITE"]);
}

#[test]
fn suites_referencing_finds_only_suite_modules() {
    let x = build(&[
        ("lib.erl", "-module(lib).\n-export([f/0]).\nf() -> ok.\n"),
        (
            "lib_helper.erl",
            "-module(lib_helper).\n-export([go/0]).\ngo() -> lib:f().\n",
        ),
        (
            "lib_SUITE.erl",
            "-module(lib_SUITE).\n-export([t/0]).\nt() -> lib:f().\n",
        ),
        (
            "other_SUITE.erl",
            "-module(other_SUITE).\n-export([t/0]).\nt() -> ok.\n",
        ),
    ]);
    let modified: BTreeSet<ModuleName> =
        std::iter::once(ModuleName::new("lib".to_owned()).unwrap()).collect();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    let names: Vec<&str> = suites.iter().map(|s| s.module.as_str()).collect();
    // lib_helper is dropped because it doesn't end with _SUITE,
    // other_SUITE is dropped because it doesn't call lib.
    assert_eq!(names, vec!["lib_SUITE"]);
}

#[test]
fn suites_referencing_empty_when_no_modules_modified() {
    let x = build(&[(
        "vhost_SUITE.erl",
        "-module(vhost_SUITE).\n-export([t/0]).\nt() -> rabbit_db_vhost:get(x).\n",
    )]);
    let modified: BTreeSet<ModuleName> = BTreeSet::new();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    assert!(suites.is_empty());
}

#[test]
fn suites_referencing_mfas_finds_only_suites_that_call_target_mfa() {
    use backhopper_core::{Arity, FunctionName, Mfa};
    use backhopper_xref::suites_referencing_mfas;
    let x = build(&[
        (
            "lib.erl",
            "-module(lib).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> ok.\n",
        ),
        (
            "lib_SUITE.erl",
            "-module(lib_SUITE).\n-export([t/0]).\nt() -> lib:f().\n",
        ),
        (
            "lib_g_SUITE.erl",
            "-module(lib_g_SUITE).\n-export([t/0]).\nt() -> lib:g().\n",
        ),
    ]);
    let target = Mfa::new(
        ModuleName::new("lib".to_owned()).unwrap(),
        FunctionName::new("f".to_owned()).unwrap(),
        Arity::new(0),
    );
    let mut targets = BTreeSet::new();
    targets.insert(target);
    let suites = suites_referencing_mfas(&x, &targets, is_suite_module);
    let names: Vec<&str> = suites.iter().map(|s| s.module.as_str()).collect();
    assert_eq!(names, vec!["lib_SUITE"]);
}

#[test]
fn suites_referencing_multiple_modules_unions_results() {
    let x = build(&[
        (
            "lib_a.erl",
            "-module(lib_a).\n-export([f/0]).\nf() -> ok.\n",
        ),
        (
            "lib_b.erl",
            "-module(lib_b).\n-export([g/0]).\ng() -> ok.\n",
        ),
        (
            "ab_SUITE.erl",
            "-module(ab_SUITE).\n-export([t/0]).\nt() -> lib_a:f(), lib_b:g().\n",
        ),
    ]);
    let modified: BTreeSet<ModuleName> = ["lib_a", "lib_b"]
        .iter()
        .map(|n| ModuleName::new((*n).to_owned()).unwrap())
        .collect();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    assert_eq!(suites.len(), 1);
}

#[test]
fn is_suite_module_only_matches_uppercase_suite_suffix() {
    use backhopper_core::ModuleName;
    assert!(is_suite_module(
        &ModuleName::new("foo_SUITE".to_owned()).unwrap()
    ));
    assert!(!is_suite_module(
        &ModuleName::new("foo_suite".to_owned()).unwrap()
    ));
    assert!(!is_suite_module(
        &ModuleName::new("SUITE_foo".to_owned()).unwrap()
    ));
    assert!(!is_suite_module(
        &ModuleName::new("foo".to_owned()).unwrap()
    ));
}

#[test]
fn suite_ref_carries_application_when_known() {
    use backhopper_xref::ProjectLayout;
    let mut b = XrefBuilder::new().with_layout(ProjectLayout::rabbitmq_main());
    let app = ApplicationName::new("rabbit".to_owned()).unwrap();
    b.add_application(
        app,
        vec![
            (
                PathBuf::from("deps/rabbit/src/lib.erl"),
                b"-module(lib).\n-export([f/0]).\nf() -> ok.\n".to_vec(),
            ),
            (
                PathBuf::from("deps/rabbit/test/lib_SUITE.erl"),
                b"-module(lib_SUITE).\n-export([t/0]).\nt() -> lib:f().\n".to_vec(),
            ),
        ],
    )
    .unwrap();
    let x = b.build().unwrap();
    let modified: BTreeSet<ModuleName> =
        std::iter::once(ModuleName::new("lib".to_owned()).unwrap()).collect();
    let suites = suites_referencing(&x, &modified, is_suite_module);
    let suite = suites.iter().next().unwrap();
    assert_eq!(suite.application.as_ref().unwrap().as_str(), "rabbit");
}