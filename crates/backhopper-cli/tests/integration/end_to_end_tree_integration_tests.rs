// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `tree show`: one module's API
//! surface from a repo ref, rendered in the canonical snapshot format.

#![cfg(unix)]

use serde_json::Value;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::GitRepoFixture;

const RA_SERVER_V1: &str = "-module(ra_server).\n\
     -export([init/1, handle_aux/2]).\n\
     -spec init(config()) -> state().\n\
     init(C) -> C.\n\
     handle_aux(A, S) -> {A, S}.\n";

// the recover/1 spec is wide enough that the canonical printer wraps it, giving the class filter a continuation line
const RA_SERVER_V2: &str = "-module(ra_server).\n\
     -export([init/1, recover/1]).\n\
     -spec init(config()) -> ra_server_state().\n\
     init(C) -> C.\n\
     -spec recover(ra_server_state()) -> {ok, ra_server_state()} | {error, snapshot_missing} | {error, log_corrupt} | {error, term()}.\n\
     recover(S) -> {ok, S}.\n";

fn repo_with_two_tags() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/ra/src/ra_server.erl", RA_SERVER_V1);
    repo.commit("seed v1");
    repo.tag("v1");
    repo.write_file("deps/ra/src/ra_server.erl", RA_SERVER_V2);
    repo.commit("narrow the surface");
    repo.tag("v2");
    repo
}

#[test]
fn tree_show_renders_the_module_in_canonical_format() {
    let repo = repo_with_two_tags();
    let t = run([
        "tree",
        "show",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "ra_server",
    ]);
    let text = stdout(&t);
    assert!(text.starts_with("module ra_server\n"), "got: {text}");
    assert!(text.contains("  export init/1\n"));
    assert!(text.contains("  spec init/1 init(config()) -> ra_server_state()"));
    assert!(text.contains("  export recover/1\n"));
}

#[test]
fn tree_show_reads_an_older_ref() {
    let repo = repo_with_two_tags();
    let t = run([
        "tree",
        "show",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--ref",
        "v1",
        "ra_server",
    ]);
    let text = stdout(&t);
    // the v1 surface: the export v2 dropped is present, v2's addition is not
    assert!(text.contains("  export handle_aux/2\n"), "got: {text}");
    assert!(!text.contains("recover"));
}

#[test]
fn tree_show_class_filter_prints_one_entry_class() {
    let repo = repo_with_two_tags();
    let t = run([
        "tree",
        "show",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--class",
        "exports",
        "ra_server",
    ]);
    let text = stdout(&t);
    assert!(text.contains("  export init/1\n"), "got: {text}");
    assert!(!text.contains("  spec "));
}

#[test]
fn tree_show_class_filter_keeps_wrapped_continuation_lines() {
    let repo = repo_with_two_tags();
    let t = run([
        "tree",
        "show",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--class",
        "specs",
        "ra_server",
    ]);
    let text = stdout(&t);
    assert!(text.contains("  spec recover/1"), "got: {text}");
    // The union wrapped: its continuation lines pass the filter.
    assert!(
        text.contains("| {error, term()}"),
        "wrapped continuation lost: {text}"
    );
    assert!(!text.contains("  export "));
}

#[test]
fn tree_show_names_the_ref_when_the_module_is_absent() {
    let repo = repo_with_two_tags();
    let assert = run([
        "tree",
        "show",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--ref",
        "v1",
        "rabbit_fifo",
    ]);
    let err = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        err.contains("rabbit_fifo") && err.contains("v1"),
        "error must name module and ref: {err}"
    );
}

#[test]
fn tree_show_json_carries_the_extracted_module() {
    let repo = repo_with_two_tags();
    let t = run([
        "--formatter",
        "json",
        "tree",
        "show",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "ra_server",
    ]);
    let env: Value = serde_json::from_str(&stdout(&t)).expect("envelope parses");
    assert_eq!(env["command"], "tree show");
    assert_eq!(env["data"]["module"], "ra_server");
    assert_eq!(env["data"]["path"], "deps/ra/src/ra_server.erl");
    let rendered = env["data"]["rendered"].as_str().expect("rendered text");
    assert!(rendered.starts_with("module ra_server\n"));
    assert!(rendered.contains("  export init/1\n"));
}
