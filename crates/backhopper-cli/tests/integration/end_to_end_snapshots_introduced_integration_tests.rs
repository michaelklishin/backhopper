// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::assert::Assert;
use serde_json::Value;
use tempfile::TempDir;

const SHA_V1_0_0: &str = "1111111111111111111111111111111111111111";
const SHA_V1_1_0: &str = "2222222222222222222222222222222222222222";
const SHA_V2_0_0: &str = "3333333333333333333333333333333333333333";

fn snapshot_body(tag: &str, commit: &str, exports_mfa_f: bool) -> String {
    let exports = if exports_mfa_f { "  export f/0\n" } else { "" };
    format!(
        "# backhopper snapshot
# format-version: 1
# project: demo
# tag: {tag}
# commit: {commit}
# scanned-paths: src/**/*.erl
# generated-by: backhopper test-fixture
# generated-at: 2026-01-01T00:00:00Z

module m
{exports}",
    )
}

struct Workspace {
    _dir: TempDir,
    cfg: PathBuf,
}

fn build_workspace() -> Workspace {
    let work = TempDir::new().unwrap();
    let snap_dir = work.path().join("snapshots");
    let demo_dir = snap_dir.join("demo");
    fs::create_dir_all(&demo_dir).unwrap();
    fs::write(
        demo_dir.join("v1.0.0.api.txt"),
        snapshot_body("v1.0.0", SHA_V1_0_0, false),
    )
    .unwrap();
    fs::write(
        demo_dir.join("v1.1.0.api.txt"),
        snapshot_body("v1.1.0", SHA_V1_1_0, true),
    )
    .unwrap();
    fs::write(
        demo_dir.join("v2.0.0.api.txt"),
        snapshot_body("v2.0.0", SHA_V2_0_0, true),
    )
    .unwrap();

    let cfg_body = format!(
        r#"config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "/tmp/demo.git"
"#,
        snap_dir.display(),
    );
    let cfg = work.path().join("backhopper.toml");
    fs::write(&cfg, cfg_body).unwrap();
    Workspace { _dir: work, cfg }
}

fn run_introduced(cfg: &Path, args: &[&str]) -> Assert {
    let mut argv = vec![
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "introduced",
        "--project",
        "demo",
        "--mfa",
        "m:f/0",
    ];
    argv.extend_from_slice(args);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(&argv)
        .assert()
}

#[test]
fn introduced_reports_first_and_last_tags_with_matching_commit_shas() {
    let ws = build_workspace();
    let assert = run_introduced(&ws.cfg, &["--formatter", "json"]);
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON envelope");

    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["command"], "snapshots introduced");
    assert_eq!(v["exit_code"], 0);

    let row = &v["data"]["rows"][0];
    assert_eq!(row["mfa"], "m:f/0");
    assert_eq!(row["first_tag"], "v1.1.0");
    assert_eq!(row["first_commit"], SHA_V1_1_0);
    assert_eq!(row["last_tag"], "v2.0.0");
    assert_eq!(row["last_commit"], SHA_V2_0_0);
    assert_eq!(row["tags_present"], 2);
    assert!(row.get("timeline").is_none(), "no --timeline by default");
}

#[test]
fn introduced_timeline_includes_every_tag_in_version_order() {
    let ws = build_workspace();
    let assert = run_introduced(&ws.cfg, &["--timeline", "--formatter", "json"]);
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&stdout).unwrap();

    let timeline = v["data"]["rows"][0]["timeline"].as_array().unwrap();
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0]["tag"], "v1.0.0");
    assert_eq!(timeline[0]["commit"], SHA_V1_0_0);
    assert_eq!(timeline[0]["present"], false);
    assert_eq!(timeline[1]["tag"], "v1.1.0");
    assert_eq!(timeline[1]["present"], true);
    assert_eq!(timeline[2]["tag"], "v2.0.0");
    assert_eq!(timeline[2]["present"], true);
}

#[test]
fn introduced_text_format_shows_short_shas_at_endpoints() {
    let ws = build_workspace();
    let assert = run_introduced(&ws.cfg, &["--formatter", "text"]);
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("m:f/0"), "text output: {stdout}");
    assert!(stdout.contains("v1.1.0"), "text output: {stdout}");
    assert!(stdout.contains("v2.0.0"), "text output: {stdout}");
    assert!(
        stdout.contains(&SHA_V1_1_0[..7]),
        "short SHA of first: {stdout}"
    );
    assert!(
        stdout.contains(&SHA_V2_0_0[..7]),
        "short SHA of last: {stdout}"
    );
}

#[test]
fn introduced_exits_nonzero_when_no_stored_tag_has_the_symbol() {
    let ws = build_workspace();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            ws.cfg.to_str().unwrap(),
            "snapshots",
            "introduced",
            "--project",
            "demo",
            "--mfa",
            "m:absent/0",
            "--formatter",
            "json",
        ])
        .assert()
        .failure();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["exit_code"], 1);
    assert_eq!(v["data"]["rows"][0]["tags_present"], 0);
    assert!(v["data"]["rows"][0]["first_tag"].is_null());
}

#[test]
fn introduced_skips_hidden_modules_unless_include_hidden_is_set() {
    let work = TempDir::new().unwrap();
    let snap_dir = work.path().join("snapshots");
    let demo_dir = snap_dir.join("demo");
    fs::create_dir_all(&demo_dir).unwrap();
    let hidden_snapshot = format!(
        "# backhopper snapshot
# format-version: 1
# project: demo
# tag: v1.0.0
# commit: {SHA_V1_0_0}
# scanned-paths: src/**/*.erl
# generated-by: backhopper test-fixture
# generated-at: 2026-01-01T00:00:00Z

module m
  visibility hidden
  export f/0
",
    );
    fs::write(demo_dir.join("v1.0.0.api.txt"), hidden_snapshot).unwrap();
    let cfg = work.path().join("backhopper.toml");
    fs::write(
        &cfg,
        format!(
            r#"config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "/tmp/demo.git"
"#,
            snap_dir.display(),
        ),
    )
    .unwrap();

    let default = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "json",
            "snapshots",
            "introduced",
            "--project",
            "demo",
            "--mfa",
            "m:f/0",
        ])
        .assert()
        .failure();
    let v: Value =
        serde_json::from_str(&String::from_utf8(default.get_output().stdout.clone()).unwrap())
            .unwrap();
    assert_eq!(v["data"]["rows"][0]["tags_present"], 0);

    let with_hidden = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "json",
            "snapshots",
            "introduced",
            "--project",
            "demo",
            "--include-hidden",
            "--mfa",
            "m:f/0",
        ])
        .assert()
        .success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(with_hidden.get_output().stdout.clone()).unwrap())
            .unwrap();
    assert_eq!(v["data"]["rows"][0]["tags_present"], 1);
    assert_eq!(v["data"]["rows"][0]["first_tag"], "v1.0.0");
}

#[test]
fn introduced_errors_when_project_has_no_stored_snapshots() {
    let work = TempDir::new().unwrap();
    let snap_dir = work.path().join("snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    let cfg = work.path().join("backhopper.toml");
    fs::write(
        &cfg,
        format!(
            r#"config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name    = "empty"
git_url = "/tmp/empty.git"
"#,
            snap_dir.display(),
        ),
    )
    .unwrap();

    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "introduced",
            "--project",
            "empty",
            "--mfa",
            "m:f/0",
        ])
        .assert()
        .failure();
}
