// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The cowlib 2.17.1 cascade in miniature: a commit whose whole
//! content is a `rabbitmq-components.mk` pin bump. The verdict stays
//! `inapplicable`, and the evidence answers the go/no-go questions: which
//! pin moved, and does the store have the bumped-to snapshot.
//! Detection is stored in the verdict cache; assessment must not be.

use std::fs;
use std::path::PathBuf;
use std::process::Command as Std;

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, run_with_env, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const DEP_V1: &str = "-module(dep_mod).\n-export([old/1]).\nold(X) -> X.\n";
const DEP_V2: &str = "-module(dep_mod).\n-export([old/1, new/1]).\nold(X) -> X.\nnew(X) -> X.\n";

fn head_sha(repo: &GitRepoFixture) -> String {
    let out = Std::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.dir.path())
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    _dep: GitRepoFixture,
    user: GitRepoFixture,
    work: TempDir,
    cfg: PathBuf,
    snapshots: PathBuf,
    bump_sha: String,
    unsnapshotted_sha: String,
    untracked_sha: String,
}

fn build_fixture() -> Fixture {
    let work = TempDir::new().unwrap();
    let dep = GitRepoFixture::new();
    dep.write_file("src/dep_mod.erl", DEP_V1);
    dep.commit("dep v1");
    dep.tag("v1.0.0");
    dep.write_file("src/dep_mod.erl", DEP_V2);
    dep.commit("dep v2");
    dep.tag("v2.0.0");

    let user = GitRepoFixture::new();
    user.write_file("rabbitmq-components.mk", "dep_dep = hex 1.0.0\n");
    user.commit("base pins");
    user.write_file("rabbitmq-components.mk", "dep_dep = hex 2.0.0\n");
    user.commit("bump dep to 2.0.0");
    let bump_sha = head_sha(&user);
    user.write_file("rabbitmq-components.mk", "dep_dep = hex 3.0.0\n");
    user.commit("bump dep to 3.0.0");
    let unsnapshotted_sha = head_sha(&user);
    user.write_file(
        "rabbitmq-components.mk",
        "dep_dep = hex 3.0.0\ndep_unknown = hex 9.9.9\n",
    );
    user.commit("introduce unknown dep");
    let untracked_sha = head_sha(&user);

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
name = "s"
pins = [{{ project = "dep", tag = "v1.0.0" }}]
"#,
        toml_path(&snapshots),
        toml_path(dep.dir.path()),
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
        work,
        cfg,
        snapshots,
        bump_sha,
        unsnapshotted_sha,
        untracked_sha,
    }
}

fn check_json(f: &Fixture, sha: &str, extra: &[&str], env: &[(&str, &str)]) -> (Value, i32) {
    let mut args = vec![
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "commit",
        "--series",
        "s",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    args.push(sha);
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

fn pin_bumps(body: &Value) -> &Vec<Value> {
    body["data"]["diagnostics"]["pin_bumps"]
        .as_array()
        .expect("pin_bumps present")
}

#[test]
fn bump_only_commit_stays_inapplicable_with_evidence() {
    let f = build_fixture();
    let (body, exit) = check_json(&f, &f.bump_sha, &[], &[]);
    assert_eq!(exit, 0);
    let row = &body["data"]["results"]["results"][0];
    assert_eq!(row["verdict"]["verdict"], "inapplicable");
    assert_eq!(row["verdict"]["reason"]["reason"], "only_makefile_touched");
    let bumps = pin_bumps(&body);
    assert_eq!(bumps.len(), 1);
    assert_eq!(bumps[0]["dep"], "dep");
    assert_eq!(bumps[0]["from"], "hex 1.0.0");
    assert_eq!(bumps[0]["to"], "hex 2.0.0");
    assert_eq!(bumps[0]["status"]["state"], "snapshot_present");
}

#[test]
fn bump_to_an_unsnapshotted_version_names_the_remedy() {
    let f = build_fixture();
    let (body, exit) = check_json(&f, &f.unsnapshotted_sha, &[], &[]);
    assert_eq!(exit, 0);
    let bumps = pin_bumps(&body);
    assert_eq!(bumps[0]["status"]["state"], "snapshot_missing");
    let note = bumps[0]["status"]["note"].as_str().unwrap();
    assert!(
        note.contains("snapshots generate --project dep --since v3.0.0"),
        "{note}"
    );
}

#[test]
fn introduced_pin_of_an_untracked_dep_reports_untracked() {
    let f = build_fixture();
    let (body, _) = check_json(&f, &f.untracked_sha, &[], &[]);
    let bumps = pin_bumps(&body);
    let unknown = bumps
        .iter()
        .find(|b| b["dep"] == "unknown")
        .expect("unknown dep bump");
    assert!(unknown["from"].is_null());
    assert_eq!(unknown["to"], "hex 9.9.9");
    assert_eq!(unknown["status"]["state"], "untracked");
}

// detection is cached, assessment is not: the same cached entry must report fresh store state
#[test]
fn cached_bump_reassesses_against_the_current_store() {
    let f = build_fixture();
    let cache_on = [("BACKHOPPER_FORCE_CACHE", "1")];
    let v2 = f.snapshots.join("dep").join("v2.0.0.api.txt");
    let parked = f.snapshots.join("v2.0.0.api.txt.parked");
    fs::rename(&v2, &parked).unwrap();

    let (body, _) = check_json(&f, &f.bump_sha, &[], &cache_on);
    assert_eq!(pin_bumps(&body)[0]["status"]["state"], "snapshot_missing");

    fs::rename(&parked, &v2).unwrap();
    let (body, _) = check_json(&f, &f.bump_sha, &[], &cache_on);
    assert_eq!(pin_bumps(&body)[0]["status"]["state"], "snapshot_present");
}

#[test]
fn auto_generate_writes_the_bumped_to_snapshot() {
    let f = build_fixture();
    let v2 = f.snapshots.join("dep").join("v2.0.0.api.txt");
    fs::remove_file(&v2).unwrap();
    let (body, _) = check_json(&f, &f.bump_sha, &["--auto-generate"], &[]);
    assert_eq!(pin_bumps(&body)[0]["status"]["state"], "snapshot_present");
    assert!(v2.exists(), "bumped-to snapshot regenerated on disk");
}

#[test]
fn auto_generate_cannot_invent_a_missing_dep_tag() {
    let f = build_fixture();
    // v3.0.0 does not exist in the dep repo: best-effort generation must keep the snapshot_missing answer
    let (body, _) = check_json(&f, &f.unsnapshotted_sha, &["--auto-generate"], &[]);
    assert_eq!(pin_bumps(&body)[0]["status"]["state"], "snapshot_missing");
}

#[test]
fn text_rendering_names_the_bump_and_status() {
    let f = build_fixture();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "check",
        "commit",
        "--series",
        "s",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        &f.bump_sha,
    ]));
    assert!(
        out.contains("pin bump: dep hex 1.0.0 -> hex 2.0.0 (snapshot: present)"),
        "{out}"
    );
}

#[test]
fn env_var_supplies_the_repo_path() {
    let f = build_fixture();
    let args = [
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "commit",
        "--series",
        "s",
        &f.bump_sha,
    ];
    let env = [(
        "BACKHOPPER_REPO_DIR_PATH",
        f.user.dir.path().to_str().unwrap(),
    )];
    let assert = run_with_env(args, &env);
    let output = assert.get_output().clone();
    assert_eq!(output.status.code(), Some(0));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(pin_bumps(&body)[0]["to"], "hex 2.0.0");
}

#[test]
fn the_flag_beats_the_env_var() {
    let f = build_fixture();
    let env = [("BACKHOPPER_REPO_DIR_PATH", "/nonexistent/nowhere")];
    let (body, exit) = check_json(&f, &f.bump_sha, &[], &env);
    assert_eq!(exit, 0);
    assert_eq!(pin_bumps(&body)[0]["to"], "hex 2.0.0");
}

// doctor's side of staleness reporting: the pin trails what the store already has
#[test]
fn doctor_reports_the_store_newest_tag_and_the_sync_nag() {
    let f = build_fixture();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "doctor",
    ]));
    let body: Value = serde_json::from_str(&out).unwrap();
    let pin = &body["data"]["series"][0]["pins"][0];
    assert_eq!(pin["store_newest_tag"], "v2.0.0");
    let note = pin["note"].as_str().unwrap();
    assert!(note.contains("series sync diff"), "{note}");
    assert!(note.contains("v2.0.0"), "{note}");
}

// compact check batch rows must still carry the bump evidence in text mode
#[test]
fn batch_text_rows_carry_the_bump_line() {
    let f = build_fixture();
    let commits = f.work.path().join("commits.txt");
    fs::write(&commits, format!("{}\n", f.bump_sha)).unwrap();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "check",
        "batch",
        "--series",
        "s",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]));
    assert!(
        out.contains("pin bump: dep hex 1.0.0 -> hex 2.0.0 (snapshot: present)"),
        "{out}"
    );
}

// check patch has no repo: detection must work from the patch bytes alone
#[test]
fn patch_file_input_detects_the_bump_without_a_repo() {
    let f = build_fixture();
    let patch = f.work.path().join("bump.patch");
    fs::write(
        &patch,
        "diff --git a/rabbitmq-components.mk b/rabbitmq-components.mk\n\
         --- a/rabbitmq-components.mk\n\
         +++ b/rabbitmq-components.mk\n\
         @@ -1,1 +1,1 @@\n\
         -dep_dep = hex 1.0.0\n\
         +dep_dep = hex 2.0.0\n",
    )
    .unwrap();
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "patch",
        "--series",
        "s",
        patch.to_str().unwrap(),
    ]));
    let body: Value = serde_json::from_str(&out).unwrap();
    let bumps = pin_bumps(&body);
    assert_eq!(bumps[0]["to"], "hex 2.0.0");
    assert_eq!(bumps[0]["status"]["state"], "snapshot_present");
}

// check range builds its diff through a second code path: the shared analyzable_diff_path predicate keeps it bump-aware
#[test]
fn range_input_detects_the_cumulative_bump() {
    let f = build_fixture();
    let base = Std::new("git")
        .args(["rev-parse", "HEAD~3"])
        .current_dir(f.user.dir.path())
        .output()
        .unwrap();
    let base = String::from_utf8(base.stdout).unwrap().trim().to_owned();
    let range = format!("{base}..{}", f.untracked_sha);
    let out = stdout(&run([
        "--config-file-path",
        f.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "range",
        "--series",
        "s",
        "--repo-dir-path",
        f.user.dir.path().to_str().unwrap(),
        "--range",
        &range,
    ]));
    let body: Value = serde_json::from_str(&out).unwrap();
    let bumps = pin_bumps(&body);
    let dep = bumps.iter().find(|b| b["dep"] == "dep").unwrap();
    assert_eq!(dep["from"], "hex 1.0.0");
    assert_eq!(dep["to"], "hex 3.0.0");
    let unknown = bumps.iter().find(|b| b["dep"] == "unknown").unwrap();
    assert!(unknown["from"].is_null());
}

// a version the TagName newtype rejects must surface as an unassessed bump (no status), not a wrong claim
#[test]
fn unresolvable_bump_version_stays_unassessed() {
    let f = build_fixture();
    f.user
        .write_file("rabbitmq-components.mk", "dep_dep = hex 2.0/0\n");
    f.user.commit("bump dep to a slash-mangled version");
    let sha = head_sha(&f.user);
    let (body, _) = check_json(&f, &sha, &[], &[]);
    let bumps = pin_bumps(&body);
    assert_eq!(bumps[0]["to"], "hex 2.0/0");
    assert!(bumps[0].get("status").is_none(), "{:?}", bumps[0]);
}
