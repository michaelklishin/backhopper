// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-MIT and LICENSE-APACHE for details.

//! End-to-end tests for indirect MFA references (meck expectations and
//! rpc forms) resolved against `--target-repo-dir-path`.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

fn write_config(workdir: &TempDir, repo: &GitRepoFixture, snapshot_dir: &Path) -> PathBuf {
    fs::create_dir_all(snapshot_dir).unwrap();
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["deps/**/src/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "demo-series"
pins = [
    {{ project = "demo", tag = "source-v1" }},
]
"#,
        toml_path(snapshot_dir),
        toml_path(repo.dir.path()),
    );
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

fn generate_snapshot(cfg_path: &Path) {
    run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-series",
    ])
    .success();
}

const BASE_CALLER: &str = "-module(caller).\n-export([go/0]).\ngo() -> ok.\n";

/// Source and target repos where the target's `helper` module exports
/// `present/0` only, and the source's new commit adds `added_lines` to
/// `caller.erl`.
fn fixture(workdir: &TempDir, added_lines: &str) -> (GitRepoFixture, GitRepoFixture, PathBuf) {
    let snapshot_dir = workdir.path().join("snap");
    let source = GitRepoFixture::new();
    source.write_file("deps/demo/src/caller.erl", BASE_CALLER);
    source.commit("seed caller");
    source.tag("source-v1");
    source.write_file(
        "deps/demo/src/caller.erl",
        &format!("{BASE_CALLER}{added_lines}"),
    );
    source.commit("port a test");
    let target = GitRepoFixture::new();
    target.write_file("deps/demo/src/caller.erl", BASE_CALLER);
    target.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([present/0]).\npresent() -> ok.\n",
    );
    target.commit("seed helper without missing/1");
    let cfg_path = write_config(workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);
    (source, target, cfg_path)
}

fn check_args<'a>(
    cfg_path: &'a Path,
    source: &'a GitRepoFixture,
    target: &'a GitRepoFixture,
    formatter: &'a str,
    sha: &'a str,
) -> Vec<&'a str> {
    vec![
        "--formatter",
        formatter,
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        sha,
    ]
}

// The motivating shape: a meck expectation carried through the CT
// broker rpc helper references a function the target module neither
// exports nor defines.
#[test]
fn an_rpc_wrapped_meck_expectation_on_an_absent_function_is_flagged() {
    let workdir = TempDir::new().unwrap();
    let (source, target, cfg_path) = fixture(
        &workdir,
        "t(Config, Node) ->\n\
         rabbit_ct_broker_helpers:rpc(Config, Node, meck, expect,\n\
             [helper, missing, 1, ok]).\n",
    );
    let sha = source.head_sha();
    let a = run(check_args(&cfg_path, &source, &target, "json", &sha));
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    let r = reasons
        .iter()
        .find(|r| r["kind"] == "indirect_call_undefined_on_target")
        .unwrap_or_else(|| panic!("expected indirect_call_undefined_on_target in {reasons:?}"));
    assert_eq!(r["module"], "helper");
    assert_eq!(r["function"], "missing");
    assert_eq!(r["arity"], 1);
    assert_eq!(r["via"], "meck_expect");
    // The rpc form starts on the file's fifth line.
    assert_eq!(r["line"], 5);
    let tally = &env["data"]["diagnostics"]["indirect_call_checks"];
    assert_eq!(tally["checked"], 1);
}

#[test]
fn a_mecked_function_the_target_exports_resolves_clean() {
    let workdir = TempDir::new().unwrap();
    let (source, target, cfg_path) =
        fixture(&workdir, "t() -> meck:expect(helper, present, 0, ok).\n");
    let sha = source.head_sha();
    let a = run(check_args(&cfg_path, &source, &target, "json", &sha));
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    assert!(
        !reasons
            .iter()
            .any(|r| r["kind"] == "indirect_call_undefined_on_target"),
        "an exported function must not be flagged: {reasons:?}"
    );
    let tally = &env["data"]["diagnostics"]["indirect_call_checks"];
    assert_eq!(tally["checked"], 1);
}

#[test]
fn the_text_output_names_the_form_and_renders_the_tally() {
    let workdir = TempDir::new().unwrap();
    let (source, target, cfg_path) = fixture(
        &workdir,
        "t(N, Args) ->\n\
         meck:expect(helper, missing, 1, ok),\n\
         rpc:call(N, helper, also_missing, Args).\n",
    );
    let sha = source.head_sha();
    let a = run(check_args(&cfg_path, &source, &target, "text", &sha));
    let text = stdout(&a);
    assert!(
        text.contains("helper:missing/1 undefined on target (via meck:expect"),
        "text output must name the form: {text}"
    );
    assert!(
        text.contains("indirect calls: 1 checked, 1 withheld (arity not readable)"),
        "text output must render the tally: {text}"
    );
}

// A cascade leg sums the indirect tallies across its rows and renders
// one line beside the other leg tallies.
#[test]
fn a_cascade_leg_renders_the_summed_indirect_tally() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    fs::create_dir_all(&snapshot_dir).unwrap();
    let dep = GitRepoFixture::new();
    dep.write_file("src/caller.erl", BASE_CALLER);
    dep.commit("seed caller");
    dep.tag("source-v1");
    let source = GitRepoFixture::new();
    source.write_file("deps/demo/src/caller.erl", BASE_CALLER);
    source.commit("seed vendored caller");
    source.write_file(
        "deps/demo/src/caller.erl",
        &format!("{BASE_CALLER}t() -> meck:expect(helper, missing, 1, ok).\n"),
    );
    source.commit("meck an absent function");
    let sha = source.head_sha();
    let target = GitRepoFixture::new();
    target.write_file("deps/demo/src/caller.erl", BASE_CALLER);
    target.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([present/0]).\npresent() -> ok.\n",
    );
    target.commit("seed helper without missing/1");
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "leg-a"
pins = [ {{ project = "demo", tag = "source-v1" }} ]
target_repo_dir_path = "{}"
"#,
        toml_path(&snapshot_dir),
        toml_path(dep.dir.path()),
        toml_path(target.dir.path()),
    );
    let cfg_path = workdir.path().join("backhopper.toml");
    fs::write(&cfg_path, body).unwrap();
    run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "leg-a",
    ])
    .success();
    let commits = workdir.path().join("commits.txt");
    fs::write(&commits, format!("{sha}\n")).unwrap();
    let a = run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "cascade",
        "--series",
        "leg-a",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    assert!(
        text.contains("indirect calls: 1 checked"),
        "cascade leg must render the summed tally: {text}"
    );
}
