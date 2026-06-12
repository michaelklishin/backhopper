// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The 2026-06-10 round in miniature: a commit in an untracked repo
//! area calls a tracked dep both directly and through `fun M:F/A`.
//! Against the pre-bump pin the verdict is `requires_adaptation`
//! naming the tag that satisfies it; against the post-bump pin it is
//! `compatible`; a docs-only commit stays `inapplicable`; and a
//! cached verdict reclassifies on retrieval once a later tag's
//! snapshot exists.

use std::fs;
use std::path::PathBuf;
use std::process::Command as Std;

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, run_with_env, stdout};
use crate::helpers::fixture::FixtureRepo;

const DEP_V1: &str = r#"
-module(dep_mod).
-export([old/1, multi/2]).
old(X) -> X.
multi(A, B) -> {A, B}.
"#;

const DEP_V2: &str = r#"
-module(dep_mod).
-export([old/1, multi/2, ensure/1]).
old(X) -> X.
multi(A, B) -> {A, B}.
ensure(X) -> X.
"#;

const USER_V1: &str = r#"
-module(user_mod).
-export([head/0]).
head() -> ok.
"#;

// `multi/2` exercises arity counting through a nested tuple; the fun
// ref is the round's exact miss shape.
const USER_V2: &str = r#"
-module(user_mod).
-export([head/0, go/1]).
head() -> ok.
go(X) ->
    F = fun dep_mod:ensure/1,
    Y = dep_mod:multi(X, [{a, [X], c}]),
    F(Y).
"#;

fn head_sha(repo: &FixtureRepo) -> String {
    let out = Std::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.dir.path())
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    _dep: FixtureRepo,
    user: FixtureRepo,
    _work: TempDir,
    cfg: PathBuf,
    snapshots: PathBuf,
    sha: String,
}

fn build_fixture() -> Fixture {
    let work = TempDir::new().unwrap();
    let dep = FixtureRepo::new();
    dep.write_file("src/dep_mod.erl", DEP_V1);
    dep.commit("dep v1");
    dep.tag("v1.0.0");
    dep.write_file("src/dep_mod.erl", DEP_V2);
    dep.commit("dep v2");
    dep.tag("v2.0.0");

    let user = FixtureRepo::new();
    user.write_file("deps/myapp/src/user_mod.erl", USER_V1);
    user.commit("base");
    user.write_file("deps/myapp/src/user_mod.erl", USER_V2);
    user.commit("call the dep");
    let sha = head_sha(&user);

    let snapshots = work.path().join("snapshots");
    let cfg = work.path().join("backhopper.toml");
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl", "include/**/*.hrl"]

[[project]]
name    = "dep"
git_url = "{}"

[[series]]
name = "s-old"
pins = [{{ project = "dep", tag = "v1.0.0" }}]

[[series]]
name = "s-new"
pins = [{{ project = "dep", tag = "v2.0.0" }}]
"#,
        snapshots.display(),
        dep.dir.path().display(),
    );
    fs::write(&cfg, body).unwrap();
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "dep",
    ]);
    Fixture {
        _dep: dep,
        user,
        _work: work,
        cfg,
        snapshots,
        sha,
    }
}

fn check_json(f: &Fixture, series: &str, env: &[(&str, &str)]) -> (Value, i32) {
    let args = [
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "commit",
        "--series",
        series,
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        &f.sha,
    ];
    let assert = if env.is_empty() {
        run(args)
    } else {
        run_with_env(args, env)
    };
    let output = assert.get_output().clone();
    let exit = output.status.code().unwrap();
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    (body, exit)
}

fn dep_pin_result(body: &Value) -> &Value {
    body["data"]["results"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pin"]["project"] == "dep")
        .expect("dep pin row present")
}

#[test]
fn pre_bump_pin_reports_requires_adaptation_with_the_satisfying_tag() {
    let f = build_fixture();
    let (body, exit) = check_json(&f, "s-old", &[]);
    assert_eq!(exit, 3);
    let pin = dep_pin_result(&body);
    assert_eq!(pin["verdict"]["verdict"], "requires_adaptation");
    let reasons = pin["verdict"]["reasons"].as_array().unwrap();
    let missing = reasons
        .iter()
        .find(|r| r["kind"] == "missing_symbol")
        .expect("missing_symbol reason");
    assert_eq!(missing["symbol"]["mfa"]["function"], "ensure");
    assert_eq!(missing["first_seen_at_tag"], "v2.0.0");
    assert_eq!(missing["needs_pin_at_least"]["project"], "dep");
    assert_eq!(missing["needs_pin_at_least"]["tag"], "v2.0.0");
    // The direct call and the fun ref both count as evidence.
    assert!(pin["tracked_refs"].as_u64().unwrap() >= 2);
}

#[test]
fn post_bump_pin_is_compatible_with_evidence() {
    let f = build_fixture();
    let (body, exit) = check_json(&f, "s-new", &[]);
    assert_eq!(exit, 0);
    let pin = dep_pin_result(&body);
    assert_eq!(pin["verdict"]["verdict"], "compatible");
    assert!(pin["tracked_refs"].as_u64().unwrap() >= 2);
}

#[test]
fn docs_only_commit_stays_inapplicable() {
    let f = build_fixture();
    f.user.write_file("README.md", "notes\n");
    f.user.commit("docs only");
    let sha = head_sha(&f.user);
    let docs = Fixture { sha, ..f };
    let (body, exit) = check_json(&docs, "s-old", &[]);
    assert_eq!(exit, 0);
    let pin = dep_pin_result(&body);
    assert_eq!(pin["verdict"]["verdict"], "inapplicable");
}

// The post-cache placement test: an entry stored while the later tag
// had no snapshot must reclassify on retrieval once the snapshot
// exists. The tag set is deliberately not a cache-key input.
#[test]
fn cached_verdict_reclassifies_once_a_later_snapshot_appears() {
    let f = build_fixture();
    let cache_on = [("BACKHOPPER_FORCE_CACHE", "1")];
    let v2_snapshot = f.snapshots.join("dep").join("v2.0.0.api.txt");
    let parked = f.snapshots.join("v2.0.0.api.txt.parked");
    fs::rename(&v2_snapshot, &parked).unwrap();

    let (body, exit) = check_json(&f, "s-old", &cache_on);
    assert_eq!(exit, 3);
    let pin = dep_pin_result(&body);
    assert_eq!(pin["verdict"]["verdict"], "incompatible");

    fs::rename(&parked, &v2_snapshot).unwrap();
    let (body, exit) = check_json(&f, "s-old", &cache_on);
    assert_eq!(exit, 3);
    let pin = dep_pin_result(&body);
    assert_eq!(pin["verdict"]["verdict"], "requires_adaptation");
    let reasons = pin["verdict"]["reasons"].as_array().unwrap();
    assert_eq!(reasons[0]["first_seen_at_tag"], "v2.0.0");
}

#[test]
fn text_rendering_names_the_remedy() {
    let f = build_fixture();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "check",
        "commit",
        "--series",
        "s-old",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        &f.sha,
    ]));
    assert!(
        out.contains("first appears at v2.0.0"),
        "remedy missing from text output: {out}"
    );
    assert!(out.contains("land the dep pin bump first"), "{out}");
}

#[test]
fn unattributed_paths_surface_in_diagnostics() {
    let f = build_fixture();
    let (body, _) = check_json(&f, "s-old", &[]);
    let unattributed = &body["data"]["diagnostics"]["unattributed_paths"];
    assert_eq!(
        unattributed["deps/myapp"].as_u64(),
        Some(1),
        "expected deps/myapp tally, got {unattributed}"
    );
}

// The batch loop applies the probe per row, after its own cache
// join point.
#[test]
fn batch_rows_reclassify_with_the_satisfying_tag() {
    let f = build_fixture();
    let commits = f.cfg.parent().unwrap().join("commits.txt");
    fs::write(&commits, format!("{}\n", f.sha)).unwrap();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "batch",
        "--series",
        "s-old",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]));
    let body: Value = serde_json::from_str(&out).unwrap();
    let rows = body["data"]["results"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let pin = rows[0]["verdict"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pin"]["project"] == "dep")
        .unwrap();
    assert_eq!(pin["verdict"]["verdict"], "requires_adaptation");
    let reasons = pin["verdict"]["reasons"].as_array().unwrap();
    assert_eq!(reasons[0]["first_seen_at_tag"], "v2.0.0");
}

#[test]
fn untracked_paths_render_without_any_flag() {
    let f = build_fixture();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "check",
        "commit",
        "--series",
        "s-old",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        &f.sha,
    ]));
    assert!(
        out.contains("Untracked paths (no configured project owns them):"),
        "breadcrumb missing: {out}"
    );
    assert!(out.contains("deps/myapp"), "{out}");
}
