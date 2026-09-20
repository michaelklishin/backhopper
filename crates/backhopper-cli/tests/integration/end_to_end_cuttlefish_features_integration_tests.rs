// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! C6.3 end-to-end: an introduced cuttlefish mapping whose target env
//! key nothing in the target tree's `.erl` sources reads fires
//! `schema_key_reader_missing`. C6.2 (the cuttlefish-pin version
//! floor) is covered at the core level in
//! `cuttlefish_features_unit_tests`; its CLI-side pin lookup is a
//! two-line connector exercised by hand, not duplicated here as an
//! end-to-end fixture.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

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

const MAPPING_WITHOUT_TARGET_KEY: &str =
    "{mapping, \"rabbit.old_key\", \"rabbit.old_key\", [{datatype, string}]}.\n";

fn mapping_with_target_key() -> String {
    format!(
        "{MAPPING_WITHOUT_TARGET_KEY}\
         {{mapping, \"rabbit.new_key\", \"rabbit.message_interceptors\", [{{datatype, string}}]}}.\n"
    )
}

fn build_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/rabbit/src/rabbit_fifo.erl", "-module(rabbit_fifo).\n");
    repo.write_file("priv/schema/rabbit.schema", MAPPING_WITHOUT_TARGET_KEY);
    repo.commit("baseline");
    repo.tag("v1.0.0");
    repo.write_file("priv/schema/rabbit.schema", &mapping_with_target_key());
    repo.commit("introduce a mapping");
    repo
}

fn build_target(reader_present: bool) -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    let src = if reader_present {
        "-module(rabbit_fifo).\nf() -> get_env(rabbit, message_interceptors, []).\n"
    } else {
        "-module(rabbit_fifo).\n"
    };
    repo.write_file("deps/rabbit/src/rabbit_fifo.erl", src);
    repo.write_file("priv/schema/rabbit.schema", MAPPING_WITHOUT_TARGET_KEY);
    repo.commit("target at baseline");
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

fn check_commit(round: &Round, target: &Path) -> Value {
    let _ = &round.workdir;
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
    serde_json::from_str(&stdout(&assert)).expect("envelope parses")
}

#[test]
fn reader_missing_fires_when_target_never_reads_the_key() {
    let round = set_up_round();
    let target = build_target(false);
    let env = check_commit(&round, target.dir.path());
    let has_finding = env["data"]["target_findings"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "schema_key_reader_missing");
    assert!(has_finding, "expected a reader-missing finding: {env}");
}

#[test]
fn reader_present_is_silent() {
    let round = set_up_round();
    let target = build_target(true);
    let env = check_commit(&round, target.dir.path());
    let has_finding = env["data"]["target_findings"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "schema_key_reader_missing");
    assert!(!has_finding, "expected silence: {env}");
}
