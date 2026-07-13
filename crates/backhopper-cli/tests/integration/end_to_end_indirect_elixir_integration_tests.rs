// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-MIT and LICENSE-APACHE for details.

//! End-to-end tests for the `#16854` shape: an Elixir CLI backport that
//! calls a function absent on the target branch through
//! `rabbit_misc:rpc_call`. The finding names the missing Erlang symbol
//! and drives the exit code, where before the `.ex` diff carried only
//! the generic `unsupported_file_type` note.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const BASE_CALLER: &str = "-module(caller).\n-export([go/0]).\ngo() -> ok.\n";

// present on the target tree, exporting list/1 only: list/0 came from a
// server-side commit never backported to this branch
const TARGET_PLUGINS: &str = "-module(rabbit_plugins).\n-export([list/1]).\nlist(_) -> [].\n";

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

/// Source with a tagged Erlang baseline, then a commit that adds an
/// Elixir file. The target exports `plugins_source` extra `.erl` files
/// through `plugins_target`, none of which define `list/0`.
fn fixture(
    workdir: &TempDir,
    ex_body: &str,
    extra_erl: Option<(&str, &str)>,
    plugins_target: &str,
) -> (GitRepoFixture, GitRepoFixture, PathBuf) {
    let snapshot_dir = workdir.path().join("snap");
    let source = GitRepoFixture::new();
    source.write_file("deps/demo/src/caller.erl", BASE_CALLER);
    source.commit("seed caller");
    source.tag("source-v1");
    source.write_file(
        "deps/rabbitmq_cli/lib/rabbitmq/cli/plugins/plugins_helpers.ex",
        ex_body,
    );
    if let Some((path, body)) = extra_erl {
        source.write_file(path, body);
    }
    source.commit("port the plugins CLI change");
    let target = GitRepoFixture::new();
    target.write_file("deps/demo/src/caller.erl", BASE_CALLER);
    target.write_file("deps/demo/src/rabbit_plugins.erl", plugins_target);
    target.commit("seed target without list/0");
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

const RPC_CALL_EX: &str = "defmodule PluginHelpers do\n\
     def list(opts) do\n\
       :rabbit_misc.rpc_call(node(opts), :rabbit_plugins, :list, [])\n\
     end\n\
   end\n";

#[test]
fn an_elixir_rpc_call_on_an_absent_function_names_the_symbol_and_exits_three() {
    let workdir = TempDir::new().unwrap();
    let (source, target, cfg_path) = fixture(&workdir, RPC_CALL_EX, None, TARGET_PLUGINS);
    let sha = source.head_sha();
    let assert = run(check_args(&cfg_path, &source, &target, "json", &sha));
    let code = assert.get_output().status.code().unwrap();
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let data = &env["data"];
    let reason = &data["target_findings"]["reasons"][0];
    assert_eq!(reason["kind"], "indirect_call_undefined_on_target");
    assert_eq!(reason["module"], "rabbit_plugins");
    assert_eq!(reason["function"], "list");
    assert_eq!(reason["arity"], 0);
    assert_eq!(reason["via"], "rabbit_misc_rpc_call");
    assert_eq!(code, 3);
    assert_eq!(env["exit_code"], 3);
}

// The target exports the function, so the axis runs and clears it: no
// symbol finding, even though the `.ex` file still carries the generic
// unsupported-file note.
#[test]
fn an_elixir_rpc_call_the_target_exports_produces_no_finding() {
    let workdir = TempDir::new().unwrap();
    let exports_list0 =
        "-module(rabbit_plugins).\n-export([list/0, list/1]).\nlist() -> [].\nlist(_) -> [].\n";
    let (source, target, cfg_path) = fixture(&workdir, RPC_CALL_EX, None, exports_list0);
    let sha = source.head_sha();
    let assert = run(check_args(&cfg_path, &source, &target, "json", &sha));
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let findings = &env["data"]["target_findings"]["reasons"];
    assert!(
        findings.as_array().is_none_or(|a| a
            .iter()
            .all(|r| r["kind"] != "indirect_call_undefined_on_target")),
        "an exported function must not be flagged: {findings}"
    );
    let tally = &env["data"]["diagnostics"]["indirect_call_checks"];
    assert_eq!(tally["checked"], 1);
}

// A commit touching one .erl and one .ex file sums the tally across both
// languages and reports both findings.
#[test]
fn a_mixed_erlang_and_elixir_commit_reports_both_findings() {
    let workdir = TempDir::new().unwrap();
    let extra = (
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\n\
         go() -> rabbit_misc:rpc_call(node(), rabbit_plugins, enabled_plugins, []).\n",
    );
    let (source, target, cfg_path) = fixture(&workdir, RPC_CALL_EX, Some(extra), TARGET_PLUGINS);
    let sha = source.head_sha();
    let assert = run(check_args(&cfg_path, &source, &target, "json", &sha));
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let data = &env["data"];
    let functions: Vec<String> = data["target_findings"]["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["kind"] == "indirect_call_undefined_on_target")
        .filter_map(|r| r["function"].as_str().map(str::to_owned))
        .collect();
    assert!(
        functions.contains(&"list".to_owned()),
        "the Elixir site must be present: {functions:?}"
    );
    assert!(
        functions.contains(&"enabled_plugins".to_owned()),
        "the Erlang site must be present: {functions:?}"
    );
    let tally = &data["diagnostics"]["indirect_call_checks"];
    assert_eq!(tally["checked"], 2, "the tally sums both languages");
}

// A combined backport adds the Erlang function and the Elixir caller in
// one commit, so the reference resolves against the patch and is not
// flagged, even though the target branch does not yet define it.
#[test]
fn a_function_the_same_commit_adds_in_an_erl_file_is_not_flagged() {
    let workdir = TempDir::new().unwrap();
    let extra = (
        "deps/demo/src/rabbit_plugins.erl",
        "-module(rabbit_plugins).\n-export([list/0, list/1]).\nlist() -> [].\nlist(_) -> [].\n",
    );
    let (source, target, cfg_path) = fixture(&workdir, RPC_CALL_EX, Some(extra), TARGET_PLUGINS);
    let sha = source.head_sha();
    let assert = run(check_args(&cfg_path, &source, &target, "json", &sha));
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let findings = &env["data"]["target_findings"]["reasons"];
    assert!(
        findings.as_array().is_none_or(|a| a
            .iter()
            .all(|r| r["kind"] != "indirect_call_undefined_on_target")),
        "a function the commit adds must not be flagged: {findings}"
    );
}

#[test]
fn the_text_output_names_the_rpc_form() {
    let workdir = TempDir::new().unwrap();
    let (source, target, cfg_path) = fixture(&workdir, RPC_CALL_EX, None, TARGET_PLUGINS);
    let sha = source.head_sha();
    let assert = run(check_args(&cfg_path, &source, &target, "text", &sha));
    let text = stdout(&assert);
    assert!(
        text.contains("rabbit_plugins:list/0 undefined on target (via rabbit_misc:rpc_call"),
        "text output must name the form: {text}"
    );
}
