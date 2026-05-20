// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::{ApplicationName, ModuleName};
use backhopper_xref::{XrefBuilder, diff_xrefs};

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
fn diff_of_xref_with_itself_is_empty() {
    let x = build(&[("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n")]);
    let d = diff_xrefs(&x, &x);
    assert!(d.added_modules.is_empty());
    assert!(d.removed_modules.is_empty());
    assert!(d.added_exports.is_empty());
    assert!(d.removed_exports.is_empty());
    assert!(d.added_calls.is_empty());
    assert!(d.removed_calls.is_empty());
}

#[test]
fn added_module_appears_in_diff() {
    let from = build(&[("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n")]);
    let to = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let d = diff_xrefs(&from, &to);
    assert!(
        d.added_modules
            .contains(&ModuleName::new("b".to_owned()).unwrap())
    );
}

#[test]
fn removed_export_appears_in_diff() {
    let from = build(&[(
        "a.erl",
        "-module(a).\n-export([go/0, drop/0]).\ngo() -> ok.\ndrop() -> ok.\n",
    )]);
    let to = build(&[("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n")]);
    let d = diff_xrefs(&from, &to);
    let names: Vec<String> = d
        .removed_exports
        .iter()
        .map(|m| m.function.as_str().to_owned())
        .collect();
    assert!(names.contains(&"drop".to_owned()));
}

#[test]
fn newly_deprecated_function_appears_in_diff() {
    let from = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let to = build(&[(
        "a.erl",
        "-module(a).\n-export([f/0]).\n-deprecated([{f, 0}]).\nf() -> ok.\n",
    )]);
    let d = diff_xrefs(&from, &to);
    assert_eq!(d.newly_deprecated.len(), 1);
}

#[test]
fn added_call_appears_in_diff() {
    let from = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let to = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let d = diff_xrefs(&from, &to);
    assert_eq!(d.added_calls.len(), 1);
    assert!(d.removed_calls.is_empty());
}

#[test]
fn un_deprecated_appears_in_diff_when_attribute_removed() {
    let from = build(&[(
        "a.erl",
        "-module(a).\n-export([f/0]).\n-deprecated([{f, 0}]).\nf() -> ok.\n",
    )]);
    let to = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let d = diff_xrefs(&from, &to);
    assert_eq!(d.un_deprecated.len(), 1);
    assert!(d.newly_deprecated.is_empty());
}

#[test]
fn removed_module_appears_in_diff() {
    let from = build(&[
        ("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n"),
        ("b.erl", "-module(b).\n-export([g/0]).\ng() -> ok.\n"),
    ]);
    let to = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let d = diff_xrefs(&from, &to);
    assert!(
        d.removed_modules
            .contains(&ModuleName::new("b".to_owned()).unwrap())
    );
}

#[test]
fn removed_call_appears_in_diff_when_caller_drops_reference() {
    let from = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let to = build(&[
        ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> ok.\n"),
        ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
    ]);
    let d = diff_xrefs(&from, &to);
    assert!(d.added_calls.is_empty());
    assert_eq!(d.removed_calls.len(), 1);
}

#[test]
fn removed_behaviour_implementation_appears_in_diff() {
    let from = build(&[(
        "a.erl",
        "-module(a).\n-behaviour(gen_server).\n-export([f/0]).\nf() -> ok.\n",
    )]);
    let to = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let d = diff_xrefs(&from, &to);
    assert_eq!(d.removed_behaviour_impls.len(), 1);
}

#[test]
fn added_export_appears_in_diff() {
    let from = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let to = build(&[(
        "a.erl",
        "-module(a).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> ok.\n",
    )]);
    let d = diff_xrefs(&from, &to);
    let names: Vec<String> = d
        .added_exports
        .iter()
        .map(|m| m.function.as_str().to_owned())
        .collect();
    assert!(names.contains(&"g".to_owned()));
}

#[test]
fn diff_with_only_changes_in_function_body_records_no_export_delta() {
    let from = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let to = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> error.\n")]);
    let d = diff_xrefs(&from, &to);
    assert!(d.added_exports.is_empty());
    assert!(d.removed_exports.is_empty());
}

#[test]
fn added_behaviour_implementation_appears_in_diff() {
    let from = build(&[("a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n")]);
    let to = build(&[(
        "a.erl",
        "-module(a).\n-behaviour(gen_server).\n-export([f/0]).\nf() -> ok.\n",
    )]);
    let d = diff_xrefs(&from, &to);
    assert_eq!(d.added_behaviour_impls.len(), 1);
}