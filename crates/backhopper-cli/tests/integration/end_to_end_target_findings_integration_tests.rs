// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end coverage of row-level target findings: a suite-only
//! candidate whose meck expectation names a function absent on the
//! target must surface the finding through the row, the clearance,
//! and the exit code even though every pin verdict is inapplicable.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

use crate::helpers::cli::{run, run_succeeds, run_with_env, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const SRC_MODULE: &str =
    "-module(rabbit_fifo).\n-export([apply/3]).\napply(_, _, State) -> State.\n";

// present only on the target tree, so it resolves live, not via snapshot
const TARGET_HELPER: &str = "-module(rabbit_maintenance).\n-export([status/0]).\nstatus() -> ok.\n";

const SUITE_V1: &str = "-module(maintenance_mode_SUITE).\n-export([all/0]).\nall() -> [t_drain].\n";

const SUITE_WITH_MECK: &str = "-module(maintenance_mode_SUITE).\n\
     -export([all/0]).\n\
     all() -> [t_drain].\n\
     t_drain(Config) ->\n\
         meck:expect(rabbit_maintenance, drain, 1, ok),\n\
         Config.\n";

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

/// Source repo: a tagged baseline, then a suite-only candidate that
/// mecks a function the target's helper never defines.
fn build_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    repo.write_file("test/maintenance_mode_SUITE.erl", SUITE_V1);
    repo.commit("baseline");
    repo.tag("v1.0.0");
    repo.write_file("test/maintenance_mode_SUITE.erl", SUITE_WITH_MECK);
    repo.commit("meck a drained node");
    repo
}

/// Target at the source baseline plus the helper module without
/// `drain/1`; the suite hunk applies cleanly, so the apply axis stays
/// out of the way.
fn build_target() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    repo.write_file("src/rabbit_maintenance.erl", TARGET_HELPER);
    repo.write_file("test/maintenance_mode_SUITE.erl", SUITE_V1);
    repo.commit("target at baseline");
    repo
}

fn generate_snapshots(cfg: &Path) {
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
}

struct Round {
    workdir: TempDir,
    cfg: PathBuf,
    source: GitRepoFixture,
    sha: String,
}

fn set_up_round() -> Round {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    generate_snapshots(&cfg);
    let sha = source.head_sha();
    Round {
        workdir,
        cfg,
        source,
        sha,
    }
}

fn check_commit(round: &Round, target: Option<&Path>, formatter: &str) -> (String, i32) {
    let mut args: Vec<String> = vec![
        "--formatter".into(),
        formatter.into(),
        "--config-file-path".into(),
        round.cfg.to_str().unwrap().into(),
        "check".into(),
        "commit".into(),
        "--series".into(),
        "stable".into(),
        "--repo-dir-path".into(),
        round.source.dir.path().to_str().unwrap().into(),
    ];
    if let Some(target) = target {
        args.push("--target-repo-dir-path".into());
        args.push(target.to_str().unwrap().into());
    }
    args.push(round.sha.clone());
    let assert = run(args);
    let code = assert.get_output().status.code().unwrap();
    (stdout(&assert), code)
}

fn check_commit_json(round: &Round, target: Option<&Path>) -> (Value, i32) {
    let (out, code) = check_commit(round, target, "json");
    let env: Value = serde_json::from_str(&out).expect("envelope parses");
    (env, code)
}

#[test]
fn a_suite_only_finding_survives_an_inapplicable_verdict() {
    let round = set_up_round();
    let target = build_target();
    let (env, code) = check_commit_json(&round, Some(target.dir.path()));
    let data = &env["data"];
    let pin = &data["results"]["results"][0];
    assert_eq!(
        pin["verdict"]["verdict"], "inapplicable",
        "suite-only diff should stay inapplicable, got: {}",
        pin["verdict"]
    );
    let reason = &data["target_findings"]["reasons"][0];
    assert_eq!(reason["kind"], "indirect_call_undefined_on_target");
    assert_eq!(reason["module"], "rabbit_maintenance");
    assert_eq!(reason["function"], "drain");
    assert_eq!(reason["arity"], 1);
    assert_eq!(code, 3, "a target finding must exit NEEDS_ATTENTION");
    assert_eq!(env["exit_code"], 3);
}

#[test]
fn without_a_target_context_the_findings_are_absent_and_exit_is_zero() {
    let round = set_up_round();
    let (env, code) = check_commit_json(&round, None);
    assert!(
        env["data"].get("target_findings").is_none(),
        "no target context, so the symbol axis must be absent: {}",
        env["data"]
    );
    assert_eq!(code, 0);
}

#[test]
fn single_check_text_output_names_the_finding() {
    let round = set_up_round();
    let target = build_target();
    let (out, code) = check_commit(&round, Some(target.dir.path()), "text");
    assert!(
        out.contains(
            "target finding: rabbit_maintenance:drain/1 undefined on target (via meck:expect"
        ),
        "text output must explain the non-zero exit, got:\n{out}"
    );
    assert_eq!(code, 3);
}

// on a row with applicable pins the pin table already shows the
// reasons, so the row-level line stays silent
#[test]
fn an_applicable_row_does_not_print_the_findings_twice() {
    let workdir = TempDir::new().unwrap();
    let source = GitRepoFixture::new();
    source.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    source.commit("baseline");
    source.tag("v1.0.0");
    source.write_file(
        "src/rabbit_fifo.erl",
        "-module(rabbit_fifo).\n-export([apply/3]).\napply(_, _, State) ->\n\
         meck:expect(rabbit_maintenance, drain, 1, ok),\nState.\n",
    );
    source.commit("meck from a src module");
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    generate_snapshots(&cfg);
    let target = build_target();
    let sha = source.head_sha();
    let round = Round {
        workdir,
        cfg,
        source,
        sha,
    };
    let (out, code) = check_commit(&round, Some(target.dir.path()), "text");
    assert!(
        out.contains("IndirectCallUndefinedOnTarget"),
        "the pin table must carry the reason, got:\n{out}"
    );
    assert!(
        !out.contains("target finding:"),
        "an applicable row must not print the row-level line too, got:\n{out}"
    );
    assert_eq!(code, 3);
}

#[test]
fn batch_folds_the_findings_into_clearance_text_and_exit() {
    let round = set_up_round();
    let target = build_target();
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{}", round.sha).unwrap();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("target findings     : 1 row of 1 (indirect_call=1)"),
        "the clearance must state the finding, got:\n{out}"
    );
    assert!(
        out.contains(
            "target finding: rabbit_maintenance:drain/1 undefined on target (via meck:expect"
        ),
        "the row must name the finding, got:\n{out}"
    );
    assert!(
        out.contains("symbol findings on the target are present"),
        "the findings narrative must name the axis, got:\n{out}"
    );
    assert_eq!(code, 3);
    let _ = &round.workdir;
}

#[test]
fn batch_with_a_clean_target_stays_zero_domain_with_findings_coverage() {
    let round = set_up_round();
    let target = {
        let repo = GitRepoFixture::new();
        repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
        repo.write_file(
            "src/rabbit_maintenance.erl",
            "-module(rabbit_maintenance).\n-export([status/0, drain/1]).\n\
             status() -> ok.\ndrain(_) -> ok.\n",
        );
        repo.write_file("test/maintenance_mode_SUITE.erl", SUITE_V1);
        repo.commit("target that defines drain/1");
        repo
    };
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{}", round.sha).unwrap();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("target findings     : none (1 row assessed)"),
        "the clean axis must be stated, got:\n{out}"
    );
    assert!(
        out.contains("no symbol findings were resolved on the assessed target"),
        "the zero-domain narrative must claim the axis, got:\n{out}"
    );
    assert_eq!(code, 0);
}

#[test]
fn cascade_marks_the_finding_leg_in_the_matrix() {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let clean_target = {
        let repo = GitRepoFixture::new();
        repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
        repo.write_file(
            "src/rabbit_maintenance.erl",
            "-module(rabbit_maintenance).\n-export([status/0, drain/1]).\n\
             status() -> ok.\ndrain(_) -> ok.\n",
        );
        repo.write_file("test/maintenance_mode_SUITE.erl", SUITE_V1);
        repo.commit("target that defines drain/1");
        repo
    };
    let finding_target = build_target();
    let snapshot_dir = workdir.path().join("snapshots");
    fs::create_dir_all(&snapshot_dir).unwrap();
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
name = "clean-leg"
pins = [ {{ project = "demo", tag = "v1.0.0" }} ]
target_repo_dir_path = "{}"

[[series]]
name = "finding-leg"
pins = [ {{ project = "demo", tag = "v1.0.0" }} ]
target_repo_dir_path = "{}"
"#,
        toml_path(&snapshot_dir),
        toml_path(source.dir.path()),
        toml_path(clean_target.dir.path()),
        toml_path(finding_target.dir.path()),
    );
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    generate_snapshots(&cfg);
    let commits = workdir.path().join("commits.txt");
    fs::write(&commits, format!("{}\n", source.head_sha())).unwrap();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "cascade",
        "--series",
        "clean-leg,finding-leg",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("inapplicable!"),
        "the finding leg's cell must carry the marker, got:\n{out}"
    );
    assert!(
        out.contains("! symbol findings on this leg"),
        "the legend must explain the marker, got:\n{out}"
    );
    assert_eq!(code, 3, "the finding leg must drive the cascade exit");
    let _ = (&clean_target, &finding_target);
}

#[test]
fn text_summary_counts_the_findings() {
    let round = set_up_round();
    let target = build_target();
    let (out, _) = check_commit(&round, Some(target.dir.path()), "text-summary");
    let fields: Vec<&str> = out.lines().next().expect("one row").split('\t').collect();
    assert_eq!(fields[1], "inapplicable");
    // seventh column is the target-findings count
    assert_eq!(fields[6], "1");
}

// OTP modules are never on the target tree; they must not read as
// missing untracked modules
#[test]
fn resolve_untracked_modules_skips_otp_modules() {
    let workdir = TempDir::new().unwrap();
    let source = GitRepoFixture::new();
    source.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    source.commit("baseline");
    source.tag("v1.0.0");
    source.write_file(
        "src/rabbit_fifo.erl",
        "-module(rabbit_fifo).\n-export([apply/3]).\napply(N, _, State) ->\n\
         rpc:call(N, erlang, node, []),\n\
         genuinely_absent_helper:go(N),\nState.\n",
    );
    source.commit("call through rpc");
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
        "--resolve-untracked-modules",
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let missing_modules: Vec<&str> = reasons
        .iter()
        .filter(|r| r["kind"] == "untracked_module_missing")
        .filter_map(|r| r["module"].as_str())
        .collect();
    assert!(
        !missing_modules.contains(&"rpc"),
        "the OTP rpc module must not read as missing: {missing_modules:?}"
    );
    assert!(
        missing_modules.contains(&"genuinely_absent_helper"),
        "a genuinely absent module must still flag: {missing_modules:?}"
    );
}

// the verdict cache must store and restore the findings unchanged;
// the cache lives under the round's own snapshot dir
#[test]
fn a_cache_hit_restores_the_findings() {
    let round = set_up_round();
    let target = build_target();
    let run_once = || {
        let assert = run_with_env(
            [
                "--formatter",
                "json",
                "--config-file-path",
                round.cfg.to_str().unwrap(),
                "check",
                "commit",
                "--series",
                "stable",
                "--repo-dir-path",
                round.source.dir.path().to_str().unwrap(),
                "--target-repo-dir-path",
                target.dir.path().to_str().unwrap(),
                &round.sha,
            ],
            &[("BACKHOPPER_FORCE_CACHE", "1")],
        );
        let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
        env["data"]["target_findings"].clone()
    };
    let first = run_once();
    let second = run_once();
    assert_eq!(first, second);
    assert_eq!(
        first["reasons"][0]["kind"],
        "indirect_call_undefined_on_target"
    );
}

#[test]
fn markdown_output_names_the_finding() {
    let round = set_up_round();
    let target = build_target();
    let (out, code) = check_commit(&round, Some(target.dir.path()), "markdown");
    assert!(
        out.contains(
            "**Target finding:** rabbit_maintenance:drain/1 undefined on target (via meck:expect"
        ),
        "markdown output must explain the non-zero exit, got:\n{out}"
    );
    assert_eq!(code, 3);
}
