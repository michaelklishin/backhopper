// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;

use tempfile::TempDir;

use crate::helpers::cli::{run_fails, run_succeeds, run_with_stdin, stdout};
use backhopper_core::schema::CURRENT_SCHEMA_VERSION;

fn build_fixture(dir: &TempDir) {
    let root = dir.path();
    let rabbit = root.join("deps/rabbit");
    fs::create_dir_all(rabbit.join("src")).unwrap();
    fs::create_dir_all(rabbit.join("test")).unwrap();
    fs::write(
        rabbit.join("src/rabbit.app.src"),
        "{application, rabbit, [{modules, []}, {applications, [kernel]}]}.",
    )
    .unwrap();
    fs::write(rabbit.join("src/rabbit_db.erl"), "-module(rabbit_db).").unwrap();
    fs::write(
        rabbit.join("test/vhost_SUITE.erl"),
        "-module(vhost_SUITE).\ngo() -> rabbit_db:list().",
    )
    .unwrap();
}

#[test]
fn suites_plan_selects_caller_suite_for_modified_module() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    let root = dir.path().to_string_lossy().to_string();
    let a = run_succeeds([
        "suites",
        "plan",
        "--repo-dir-path",
        root.as_str(),
        "--modified-path",
        "deps/rabbit/src/rabbit_db.erl",
    ]);
    let out = stdout(&a);
    assert!(out.contains("rabbit/vhost_SUITE"));
    assert!(out.contains("SameAppCaller"));
}

#[test]
fn suites_plan_emits_json_envelope() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    let root = dir.path().to_string_lossy().to_string();
    let a = run_succeeds([
        "--formatter",
        "json",
        "suites",
        "plan",
        "--repo-dir-path",
        root.as_str(),
        "--modified-path",
        "deps/rabbit/src/rabbit_db.erl",
    ]);
    let out = stdout(&a);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(parsed["command"], "suites plan");
    assert_eq!(parsed["schema_version"], CURRENT_SCHEMA_VERSION);
    let entries = parsed["data"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["suite"]["module"], "vhost_SUITE");
}

#[test]
fn suites_plan_reads_modified_paths_from_stdin() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    let root = dir.path().to_string_lossy().to_string();
    let a = run_with_stdin(
        [
            "suites",
            "plan",
            "--repo-dir-path",
            root.as_str(),
            "--modified-paths-file-path",
            "-",
        ],
        "deps/rabbit/src/rabbit_db.erl\n",
    )
    .success();
    let out = stdout(&a);
    assert!(out.contains("rabbit/vhost_SUITE"));
}

// The management shape: a modified module no suite references. The
// plan must name the application and its candidate suites instead
// of printing a bare "no suites selected".
#[test]
fn suites_plan_reports_uncovered_application_with_candidates() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    let root = dir.path();
    fs::write(
        root.join("deps/rabbit/src/rabbit_mgmt_like.erl"),
        "-module(rabbit_mgmt_like).",
    )
    .unwrap();
    // The erlang.mk marker makes the run hint fire so this test can
    // pin that uncovered candidates trigger it too.
    fs::write(root.join("erlang.mk"), "").unwrap();
    let root = root.to_string_lossy().to_string();
    let a = run_succeeds([
        "suites",
        "plan",
        "--repo-dir-path",
        root.as_str(),
        "--modified-path",
        "deps/rabbit/src/rabbit_mgmt_like.erl",
    ]);
    let out = stdout(&a);
    assert!(out.contains("no suites selected"));
    assert!(
        out.contains("uncovered: rabbit (1 modified module(s), 1 suite(s) discovered, 0 selected)")
    );
    assert!(out.contains("candidates: vhost_SUITE"));
    // The run hint prints for uncovered candidates too.
    assert!(out.contains("run each suite"));
}

#[test]
fn suites_plan_counts_unattributed_paths_in_json() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    fs::write(dir.path().join("rabbitmq-components.mk"), "x\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let a = run_succeeds([
        "--formatter",
        "json",
        "suites",
        "plan",
        "--repo-dir-path",
        root.as_str(),
        "--modified-path",
        "rabbitmq-components.mk",
    ]);
    let out = stdout(&a);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(parsed["data"]["unattributed_paths"], 1);
}

#[test]
fn suites_plan_fails_loudly_on_broken_explicit_config() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    let bad_config = dir.path().join("broken.toml");
    fs::write(&bad_config, "config_version = 1\n[[suite_rule]]\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let a = run_fails([
        "--config-file-path",
        bad_config.to_string_lossy().as_ref(),
        "suites",
        "plan",
        "--repo-dir-path",
        root.as_str(),
        "--modified-path",
        "deps/rabbit/src/rabbit_db.erl",
    ]);
    let err = String::from_utf8(a.get_output().stderr.clone()).unwrap();
    assert!(
        !err.is_empty(),
        "expected a config error on stderr, got nothing"
    );
}

#[test]
fn suites_plan_errors_when_no_modified_input_supplied() {
    let dir = TempDir::new().unwrap();
    build_fixture(&dir);
    let root = dir.path().to_string_lossy().to_string();
    let a = run_fails(["suites", "plan", "--repo-dir-path", root.as_str()]);
    let err = String::from_utf8(a.get_output().stderr.clone()).unwrap();
    assert!(
        err.to_lowercase().contains("modified-path"),
        "expected hint about --modified-path, got: {err}"
    );
}
