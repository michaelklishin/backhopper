// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end reproduction of the HF-59 probe: a `-rabbit_boot_step`
//! attribute's `{mfa, {M, F, Args}}` field naming a function the
//! target does not export.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const SRC_MODULE: &str = "-module(rabbit).\n\
    -rabbit_boot_step({logger_exchange,\n\
        [{description, \"log exchange\"},\n\
         {mfa, {rabbit_logger_exchange_h, declare_exchange, []}},\n\
         {requires, core_initialized}]}).\n";

const HANDLER_WITHOUT_DECLARE_0: &str = "-module(rabbit_logger_exchange_h).\n\
    -export([declare_exchange/1]).\n\
    declare_exchange(_) -> ok.\n";

const HANDLER_WITH_DECLARE_0: &str = "-module(rabbit_logger_exchange_h).\n\
    -export([declare_exchange/0]).\n\
    declare_exchange() -> ok.\n";

fn write_config(dir: &Path, repo: &Path, snapshot_dir: &Path) -> PathBuf {
    fs::create_dir_all(snapshot_dir).unwrap();
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "stable"
pins = [
    {{ project = "demo", tag = "v1.0.0" }},
]
"#,
        toml_path(snapshot_dir),
        toml_path(repo),
    );
    let p = dir.join("backhopper.toml");
    fs::write(&p, body).unwrap();
    p
}

fn build_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit.erl", "-module(rabbit).\n");
    repo.commit("baseline");
    repo.tag("v1.0.0");
    repo.write_file("src/rabbit.erl", SRC_MODULE);
    repo.commit("register the logger exchange boot step");
    repo
}

fn build_target(handler: &str) -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit.erl", "-module(rabbit).\n");
    repo.write_file("src/rabbit_logger_exchange_h.erl", handler);
    repo.commit("target");
    repo
}

struct Round {
    workdir: TempDir,
    cfg: PathBuf,
    source: GitRepoFixture,
    sha: String,
}

fn set_up_round() -> Round {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let sha = source.head_sha();
    Round {
        workdir,
        cfg,
        source,
        sha,
    }
}

fn check_commit_json(round: &Round, target: &Path) -> (Value, i32) {
    let assert = run([
        "--formatter",
        "json",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.to_str().unwrap(),
        &round.sha,
    ]);
    let code = assert.get_output().status.code().unwrap();
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    (env, code)
}

#[test]
fn a_boot_step_mfa_tuple_naming_an_unexported_function_is_flagged() {
    let round = set_up_round();
    let target = build_target(HANDLER_WITHOUT_DECLARE_0);
    let (env, code) = check_commit_json(&round, target.dir.path());
    let reasons = &env["data"]["target_findings"]["reasons"];
    assert_eq!(reasons.as_array().unwrap().len(), 1, "got: {reasons}");
    let reason = &reasons[0];
    assert_eq!(reason["kind"], "qualified_call_undefined_on_target");
    assert_eq!(reason["module"], "rabbit_logger_exchange_h");
    assert_eq!(reason["function"], "declare_exchange");
    assert_eq!(reason["arity"], 0);
    assert_eq!(code, 3, "a target finding must exit NEEDS_ATTENTION");
    let _ = &round.workdir;
}

#[test]
fn the_export_present_on_the_target_is_a_negative_control() {
    let round = set_up_round();
    let target = build_target(HANDLER_WITH_DECLARE_0);
    let (env, code) = check_commit_json(&round, target.dir.path());
    let reasons = &env["data"]["target_findings"]["reasons"];
    assert!(
        reasons.as_array().unwrap().is_empty(),
        "no boot step finding expected, got: {reasons}"
    );
    assert_eq!(code, 0);
}
