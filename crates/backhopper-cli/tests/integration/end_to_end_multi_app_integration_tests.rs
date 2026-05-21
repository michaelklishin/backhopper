// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use tempfile::TempDir;

use crate::helpers::cli::{run_succeeds, stdout};
use crate::helpers::fixture::FixtureRepo;

const APP_A: &str = "-module(app_a).\n-export([go/0]).\ngo() -> ok.\n";
const APP_B: &str = "-module(app_b).\n-export([run/0]).\nrun() -> ok.\n";
const EXCLUDED: &str = "-module(excluded_mod).\n-export([f/0]).\nf() -> ok.\n";
const SUITE: &str = "-module(app_a_SUITE).\n-export([all/0]).\nall() -> [].\n";

fn write_multi_app_config(workdir: &std::path::Path, repo: &std::path::Path) -> std::path::PathBuf {
    let snap_dir = workdir.join("snapshots");
    std::fs::create_dir_all(&snap_dir).unwrap();
    let cfg_body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir = "{}"

[[project]]
name = "demo"
git_url = "{}"
layout = "multi_app"
app_roots = ["lib/*"]
exclude_apps = ["excluded"]
excluded_subdirs = ["test"]
"#,
        snap_dir.display(),
        repo.display(),
    );
    let cfg = workdir.join("backhopper.toml");
    std::fs::write(&cfg, cfg_body).unwrap();
    cfg
}

fn build_multi_app_repo() -> (FixtureRepo, TempDir) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("lib/app_a/src/app_a.erl", APP_A);
    repo.write_file("lib/app_a/test/app_a_SUITE.erl", SUITE);
    repo.write_file("lib/app_b/src/app_b.erl", APP_B);
    repo.write_file("lib/excluded/src/excluded_mod.erl", EXCLUDED);
    repo.commit("seed");
    repo.tag("v1.0.0");
    (repo, workdir)
}

#[test]
fn generate_indexes_only_included_apps_and_records_apps_scanned() {
    let (repo, workdir) = build_multi_app_repo();
    let cfg = write_multi_app_config(workdir.path(), repo.dir.path());
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let snap =
        std::fs::read_to_string(workdir.path().join("snapshots/demo/v1.0.0.api.txt")).unwrap();
    assert!(snap.contains("module app_a"));
    assert!(snap.contains("module app_b"));
    assert!(
        !snap.contains("module excluded_mod"),
        "excluded app must not appear"
    );
    assert!(
        !snap.contains("module app_a_SUITE"),
        "test/ subdir must be skipped"
    );
    let apps_line = snap
        .lines()
        .find(|l| l.starts_with("# apps-scanned:"))
        .expect("apps-scanned header");
    assert!(apps_line.contains("app_a"));
    assert!(apps_line.contains("app_b"));
    assert!(
        !apps_line.contains("excluded"),
        "excluded app must not appear in apps-scanned"
    );
}

#[test]
fn pattern_pin_resolves_against_store_at_check_time() {
    let (repo, workdir) = build_multi_app_repo();
    // Add a second tag so the pattern has more than one candidate
    repo.write_file(
        "lib/app_a/src/app_a.erl",
        "-module(app_a).\n-export([go/0, go2/0]).\ngo() -> ok.\ngo2() -> ok.\n",
    );
    repo.commit("rev");
    repo.tag("v1.1.0");
    let snap_dir = workdir.path().join("snapshots");
    std::fs::create_dir_all(&snap_dir).unwrap();
    let cfg_body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir = "{}"

[[project]]
name = "demo"
git_url = "{}"
layout = "multi_app"
app_roots = ["lib/*"]
exclude_apps = ["excluded"]

[[series]]
name = "stable"
pins = [{{ project = "demo", tag_pattern = "v1.*", select = "latest" }}]
"#,
        snap_dir.display(),
        repo.dir.path().display(),
    );
    let cfg = workdir.path().join("backhopper.toml");
    std::fs::write(&cfg, cfg_body).unwrap();
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let out = run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "series",
        "show",
        "--series",
        "stable",
    ]);
    let text = stdout(&out);
    // Series show prints the pattern form, not the resolved tag
    assert!(text.contains("v1.* (latest)"), "got: {text}");
}

#[test]
fn lookup_against_multi_app_snapshot_finds_module_in_included_app() {
    let (repo, workdir) = build_multi_app_repo();
    let cfg = write_multi_app_config(workdir.path(), repo.dir.path());
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let out = run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "lookup",
        "--project",
        "demo",
        "--tag",
        "v1.0.0",
        "--mfa",
        "app_a:go/0",
    ]);
    let text = stdout(&out);
    assert!(text.contains("found"), "got: {text}");
}
