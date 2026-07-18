// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `backhopper init`.

use tempfile::TempDir;

use crate::helpers::cli::{run, stderr, stdout};
use backhopper_core::schema::CURRENT_SCHEMA_VERSION;

#[test]
fn init_writes_a_starter_config_with_absolute_snapshot_dir() {
    let tmp = TempDir::new().unwrap();
    let a = run(["init", "--config-dir-path", tmp.path().to_str().unwrap()]);
    a.success();
    let cfg = tmp.path().join("backhopper.toml");
    assert!(cfg.exists(), "config file was not written");
    let body = std::fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("config_version = 1"));
    assert!(body.contains("snapshot_dir"));
    // the path is absolute so a relative snapshot_dir cannot resolve above the config directory
    let snapshot_dir_line = body.lines().find(|l| l.contains("snapshot_dir")).unwrap();
    assert!(
        snapshot_dir_line.contains(tmp.path().to_str().unwrap()),
        "snapshot_dir must be absolute under the tmpdir, got: {snapshot_dir_line}"
    );
}

#[test]
fn init_refuses_to_overwrite_without_force() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("backhopper.toml");
    std::fs::write(&cfg, "config_version = 1\n").unwrap();
    let a = run(["init", "--config-dir-path", tmp.path().to_str().unwrap()]).failure();
    let err = stderr(&a);
    assert!(err.contains("--force"), "stderr was: {err}");
    let preserved = std::fs::read_to_string(&cfg).unwrap();
    assert_eq!(preserved, "config_version = 1\n");
}

#[test]
fn init_force_overwrites_existing_config() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("backhopper.toml");
    std::fs::write(&cfg, "garbage = true\n").unwrap();
    let a = run([
        "init",
        "--config-dir-path",
        tmp.path().to_str().unwrap(),
        "--force",
    ]);
    a.success();
    let body = std::fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("config_version = 1"));
    assert!(!body.contains("garbage"));
}

#[test]
fn init_uses_explicit_snapshot_dir_when_provided() {
    let tmp = TempDir::new().unwrap();
    let snapshots = tmp.path().join("custom-snapshots");
    let a = run([
        "init",
        "--config-dir-path",
        tmp.path().to_str().unwrap(),
        "--snapshot-dir-path",
        snapshots.to_str().unwrap(),
    ]);
    a.success();
    let body = std::fs::read_to_string(tmp.path().join("backhopper.toml")).unwrap();
    assert!(body.contains("custom-snapshots"));
    assert!(snapshots.exists(), "snapshot directory should be created");
}

#[test]
fn init_emits_next_step_hint_in_text_output() {
    let tmp = TempDir::new().unwrap();
    let a = run(["init", "--config-dir-path", tmp.path().to_str().unwrap()]);
    let text = stdout(&a);
    assert!(text.contains("doctor"), "should hint at `doctor` next");
}

#[test]
fn init_json_envelope_carries_paths() {
    let tmp = TempDir::new().unwrap();
    let a = run([
        "--formatter",
        "json",
        "init",
        "--config-dir-path",
        tmp.path().to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["command"], "init");
    assert_eq!(v["schema_version"], CURRENT_SCHEMA_VERSION);
    assert!(
        v["data"]["wrote"]
            .as_str()
            .unwrap()
            .ends_with("backhopper.toml")
    );
}

#[test]
fn init_output_is_a_loadable_config() {
    let tmp = TempDir::new().unwrap();
    let a = run(["init", "--config-dir-path", tmp.path().to_str().unwrap()]);
    a.success();
    let cfg = tmp.path().join("backhopper.toml");
    let validate = run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "config",
        "validate",
    ]);
    validate.success();
}

#[test]
fn init_with_rabbitmq_reports_skipped_branches_that_have_no_components_mk() {
    use backhopper_test_support::GitRepoFixture;

    let repo = GitRepoFixture::new();
    repo.write_file("README.md", "no components.mk here\n");
    repo.commit("seed");

    let workdir = TempDir::new().unwrap();
    let a = run([
        "init",
        "--config-dir-path",
        workdir.path().to_str().unwrap(),
        "--rabbitmq-repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--rabbitmq-branches",
        "main",
    ])
    .success();
    let text = stdout(&a);
    assert!(
        text.contains("skipped 1 branch"),
        "stdout should report skipped branches, got: {text}"
    );
    assert!(text.contains("main"));
}

#[test]
fn init_with_rabbitmq_repo_writes_inferred_series_and_project_stubs() {
    use backhopper_test_support::GitRepoFixture;

    let components_mk = "\
dep_ra = hex 2.16.13
dep_khepri = hex 0.17.0
dep_osiris = hex 1.8.6
";
    let repo = GitRepoFixture::new();
    repo.write_file("rabbitmq-components.mk", components_mk);
    repo.commit("seed");

    let workdir = TempDir::new().unwrap();
    let a = run([
        "init",
        "--config-dir-path",
        workdir.path().to_str().unwrap(),
        "--rabbitmq-repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--rabbitmq-branches",
        "main",
    ]);
    a.success();
    let body = std::fs::read_to_string(workdir.path().join("backhopper.toml")).unwrap();
    // The components.mk pins are mapped into [[series]] + [[project]] stubs.
    assert!(body.contains("[[series]]"), "config body: {body}");
    assert!(body.contains("ra"), "config body: {body}");
    assert!(body.contains("khepri"));
    assert!(body.contains("osiris"));
    assert!(body.contains("v2.16.13"));
    assert!(body.contains("v0.17.0"));
    assert!(body.contains("git_url"));
    // The result must still load: project stubs need a non-empty git_url
    let validate = run([
        "--config-file-path",
        workdir.path().join("backhopper.toml").to_str().unwrap(),
        "config",
        "validate",
    ]);
    validate.success();
}

#[test]
fn init_with_rabbitmq_repo_emits_self_project_block() {
    use backhopper_test_support::GitRepoFixture;

    let components_mk = "dep_ra = hex 2.16.13\n";
    let repo = GitRepoFixture::new();
    repo.write_file("rabbitmq-components.mk", components_mk);
    repo.commit("seed");

    let workdir = TempDir::new().unwrap();
    let a = run([
        "init",
        "--config-dir-path",
        workdir.path().to_str().unwrap(),
        "--rabbitmq-repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--rabbitmq-branches",
        "main",
    ]);
    a.success();
    let body = std::fs::read_to_string(workdir.path().join("backhopper.toml")).unwrap();
    assert!(
        body.contains("name      = \"rabbitmq-server\""),
        "body: {body}"
    );
    assert!(body.contains("kind      = \"self\""), "body: {body}");
    assert!(body.contains("family    = \"rabbitmq\""), "body: {body}");
    assert!(body.contains("app_roots = [\"deps\"]"), "body: {body}");
    assert!(
        body.contains("project = \"rabbitmq-server\""),
        "self-project must also be pinned in [[series]] blocks; body: {body}"
    );
}
