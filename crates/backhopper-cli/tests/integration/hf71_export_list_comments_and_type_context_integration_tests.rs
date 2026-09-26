// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end reproduction of HF-71's two false positives: an export
//! that follows a comment inside the target's `-export` list, and a
//! `timeout()` added deep inside a multi-line `-type` whose opener sits
//! outside the hunk.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const CONN_BEFORE: &str = r"-module(conn).
-export([open/1]).

-type config() ::
    #{host => binary(),
      port => inet:port_number(),
      tls => boolean(),
      retries => non_neg_integer(),
      notify => pid() | none
    }.

open(Cfg) ->
    Cfg.
";

const CONN_AFTER: &str = r"-module(conn).
-export([open/1]).

-type config() ::
    #{host => binary(),
      port => inet:port_number(),
      tls => boolean(),
      retries => non_neg_integer(),
      connect_timeout => timeout(),
      notify => pid() | none
    }.

open(Cfg) ->
    rabbit_fifo:make_enqueue(self(), 1, Cfg),
    rabbit_fifo:make_gone(Cfg).
";

const TARGET_FIFO: &str = r"-module(rabbit_fifo).
-export([
         init/1,
         %% protocol helpers
         make_enqueue/3
        ]).

init(_) -> ok.
make_enqueue(_, _, _) -> ok.
";

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

#[test]
fn a_commented_export_and_a_type_line_far_from_its_opener_are_not_flagged() {
    let workdir = TempDir::new().unwrap();
    let source = GitRepoFixture::new();
    source.write_file("src/conn.erl", CONN_BEFORE);
    source.commit("baseline");
    source.tag("v1.0.0");
    source.write_file("src/conn.erl", CONN_AFTER);
    source.commit("add a connect timeout and enqueue on open");
    let target = GitRepoFixture::new();
    target.write_file("src/conn.erl", CONN_BEFORE);
    target.write_file("src/rabbit_fifo.erl", TARGET_FIFO);
    target.commit("target");

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
        target.dir.path().to_str().unwrap(),
        "--no-cache",
        &source.head_sha(),
    ]);
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let reasons = env["data"]["target_findings"]["reasons"]
        .as_array()
        .unwrap();
    let flagged: Vec<(&str, &str)> = reasons
        .iter()
        .map(|r| (r["kind"].as_str().unwrap(), r["function"].as_str().unwrap()))
        .collect();
    assert_eq!(
        flagged,
        [("qualified_call_undefined_on_target", "make_gone")],
        "only the call the target really lacks is flagged"
    );
}
