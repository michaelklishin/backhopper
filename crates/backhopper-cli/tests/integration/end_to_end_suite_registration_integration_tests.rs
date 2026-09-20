// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! C5 end-to-end: an added `_SUITE.erl` in an app whose target
//! Makefile carries `PARALLEL_CT` without registering the suite must
//! surface `suite_not_registered_for_ct` on the pin verdict itself,
//! not just the diagnostics row — the prime case doc 025 §7.2a calls
//! out, since a suite-only patch promotes to
//! `Inapplicable(OnlyTestFixturesTouched)` and the skip merge arm
//! would otherwise swallow the finding.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const MAKEFILE_WITH_MARKER: &str = "PARALLEL_CT_SET_1_A = existing_suite\n";
const MAKEFILE_WITHOUT_MARKER: &str = "all:\n\techo ok\n";
const EXISTING_SUITE: &str = "-module(existing_suite_SUITE).\n-export([all/0]).\nall() -> [].\n";
const NEW_SUITE: &str = "-module(new_thing_SUITE).\n-export([all/0]).\nall() -> [].\n";

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
family  = "rabbitmq"

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
    repo.write_file("src/rabbit_fifo.erl", "-module(rabbit_fifo).\n");
    repo.write_file("test/existing_suite_SUITE.erl", EXISTING_SUITE);
    repo.commit("baseline");
    repo.tag("v1.0.0");
    repo.write_file("test/new_thing_SUITE.erl", NEW_SUITE);
    repo.commit("add a new suite");
    repo
}

fn build_target(makefile: &str) -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit_fifo.erl", "-module(rabbit_fifo).\n");
    repo.write_file("test/existing_suite_SUITE.erl", EXISTING_SUITE);
    repo.write_file("Makefile", makefile);
    repo.commit("target at baseline");
    repo
}

fn generate_snapshots(cfg: &Path) {
    run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
}

fn check_commit(cfg: &Path, source: &GitRepoFixture, target: &Path, sha: &str) -> Value {
    let assert = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.to_str().unwrap(),
        sha,
    ]);
    serde_json::from_str(&stdout(&assert)).expect("envelope parses")
}

#[test]
fn added_suite_without_registration_surfaces_on_the_pin_verdict() {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    generate_snapshots(&cfg);
    let sha = source.head_sha();
    let target = build_target(MAKEFILE_WITH_MARKER);

    let env = check_commit(&cfg, &source, target.dir.path(), &sha);
    let pin = &env["data"]["results"]["results"][0];
    assert_ne!(
        pin["verdict"]["verdict"], "inapplicable",
        "the override merge arm must surface the finding instead of \
         leaving the promoted Inapplicable verdict in place: {}",
        pin["verdict"]
    );
    let reasons = &pin["verdict"]["reasons"];
    assert!(
        reasons
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["kind"] == "suite_not_registered_for_ct"),
        "expected suite_not_registered_for_ct, got {reasons}"
    );
}

#[test]
fn added_suite_stays_inapplicable_without_the_marker() {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    generate_snapshots(&cfg);
    let sha = source.head_sha();
    let target = build_target(MAKEFILE_WITHOUT_MARKER);

    let env = check_commit(&cfg, &source, target.dir.path(), &sha);
    let pin = &env["data"]["results"]["results"][0];
    assert_eq!(
        pin["verdict"]["verdict"], "inapplicable",
        "an app without the marker dispatches by discovery: promotion \
         must stay byte-identical to today: {}",
        pin["verdict"]
    );
}

#[test]
fn dormant_detectors_report_the_missing_target_repo() {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    generate_snapshots(&cfg);
    let sha = source.head_sha();

    let assert = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let dormant = env["data"]["diagnostics"]["dormant_detectors"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        dormant
            .iter()
            .any(|d| d["detector"] == "suite_not_registered_for_ct"),
        "expected suite_not_registered_for_ct to be reported dormant: {dormant:?}"
    );
    assert!(
        dormant
            .iter()
            .any(|d| d["detector"] == "schema_feature_unsupported_on_pin"
                && d["missing_input"] == "cuttlefish_pin"),
        "expected schema_feature_unsupported_on_pin to be reported dormant: {dormant:?}"
    );
}
