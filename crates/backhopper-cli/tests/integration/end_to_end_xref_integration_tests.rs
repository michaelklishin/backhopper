// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use assert_cmd::Command;

use crate::helpers::cli::{run, stdout};

#[test]
fn default_formatter_is_json_not_text() {
    let tmp = TempDir::new().unwrap();
    write_tree(
        tmp.path(),
        &[(
            "a.erl",
            "-module(a).\n-export([go/0]).\ngo() -> missing:f(1).\n",
        )],
    );
    // `backhopper` defaults to a machine-readable format
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .env_remove("BACKHOPPER_FORMATTER")
        .args([
            "xref",
            "list_undefined",
            "--tree-dir-path",
            tmp.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        out.trim_start().starts_with('{'),
        "default output should be JSON, got: {out}"
    );
    assert!(out.contains("\"schema_version\""));
    assert!(out.contains("missing"));
}

fn write_tree(dir: &Path, files: &[(&str, &str)]) {
    for (rel, body) in files {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, body).unwrap();
    }
}

#[test]
fn xref_list_callers_returns_caller_in_json() {
    let tmp = TempDir::new().unwrap();
    write_tree(
        tmp.path(),
        &[
            ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
            ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
        ],
    );
    let out = stdout(
        &run([
            "--formatter",
            "json",
            "xref",
            "list_callers",
            "--tree-dir-path",
            tmp.path().to_str().unwrap(),
            "--mfa",
            "b:f/0",
        ])
        .success(),
    );
    assert!(out.contains("\"caller\""));
    assert!(out.contains("\"module\": \"a\""));
    assert!(out.contains("\"function\": \"go\""));
}

#[test]
fn xref_list_undefined_returns_undefined_function_calls() {
    let tmp = TempDir::new().unwrap();
    write_tree(
        tmp.path(),
        &[(
            "a.erl",
            "-module(a).\n-export([go/0]).\ngo() -> missing:f(1).\n",
        )],
    );
    let out = stdout(
        &run([
            "xref",
            "list_undefined",
            "--tree-dir-path",
            tmp.path().to_str().unwrap(),
        ])
        .success(),
    );
    assert!(out.contains("missing"));
}

#[test]
fn xref_list_callees_returns_target_callees() {
    let tmp = TempDir::new().unwrap();
    write_tree(
        tmp.path(),
        &[
            ("a.erl", "-module(a).\n-export([go/0]).\ngo() -> b:f().\n"),
            ("b.erl", "-module(b).\n-export([f/0]).\nf() -> ok.\n"),
        ],
    );
    let out = stdout(
        &run([
            "--formatter",
            "json",
            "xref",
            "list_callees",
            "--tree-dir-path",
            tmp.path().to_str().unwrap(),
            "--mfa",
            "a:go/0",
        ])
        .success(),
    );
    assert!(out.contains("\"module\": \"b\""));
}

#[test]
fn xref_list_module_cycles_finds_two_module_cycle() {
    let tmp = TempDir::new().unwrap();
    write_tree(
        tmp.path(),
        &[
            ("a.erl", "-module(a).\n-export([f/0]).\nf() -> b:g().\n"),
            ("b.erl", "-module(b).\n-export([g/0]).\ng() -> a:f().\n"),
        ],
    );
    let out = stdout(
        &run([
            "--formatter",
            "json",
            "xref",
            "list_module_cycles",
            "--tree-dir-path",
            tmp.path().to_str().unwrap(),
        ])
        .success(),
    );
    assert!(out.contains("\"a\""));
    assert!(out.contains("\"b\""));
}

#[test]
fn suites_list_for_modules_returns_suite() {
    let tmp = TempDir::new().unwrap();
    write_tree(
        tmp.path(),
        &[
            (
                "src/lib.erl",
                "-module(lib).\n-export([f/0]).\nf() -> ok.\n",
            ),
            (
                "test/lib_SUITE.erl",
                "-module(lib_SUITE).\n-export([t/0]).\nt() -> lib:f().\n",
            ),
        ],
    );
    let out = stdout(
        &run([
            "suites",
            "list_for_modules",
            "--tree-dir-path",
            tmp.path().to_str().unwrap(),
            "--module",
            "lib",
        ])
        .success(),
    );
    assert!(out.contains("lib_SUITE"));
}
