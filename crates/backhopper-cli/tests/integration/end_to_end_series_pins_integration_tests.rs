// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use assert_cmd::Command;

use backhopper_test_support::GitRepoFixture;

const OLD_COMPONENTS_MK: &str = "\
PROJECT = rabbit
dep_cowboy = hex 2.13.0
dep_ra = hex 2.15.4
";

const NEW_COMPONENTS_MK: &str = "\
PROJECT = rabbit
dep_cowboy = hex 2.16.0
dep_ra = hex 2.15.4
dep_gun = git https://github.com/ninenines/gun 2.2.0
";

fn build_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("rabbitmq-components.mk", OLD_COMPONENTS_MK);
    repo.commit("old pins");
    repo.tag("old");
    repo.write_file("rabbitmq-components.mk", NEW_COMPONENTS_MK);
    repo.commit("bump cowboy, add gun");
    repo
}

fn run_fmt(repo: &GitRepoFixture, formatter: &str, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut args: Vec<&str> = vec![
        "--formatter",
        formatter,
        "series",
        "pins",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(&args)
        .assert()
}

fn run(repo: &GitRepoFixture, extra: &[&str]) -> assert_cmd::assert::Assert {
    run_fmt(repo, "text", extra)
}

#[test]
fn pins_lists_branch_pins_sorted_by_name() {
    let repo = build_repo();
    let out = run(&repo, &["--branch", "main"])
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "cowboy\thex\t2.16.0");
    assert_eq!(lines[1], "gun\tgit\t2.2.0");
    assert_eq!(lines[2], "ra\thex\t2.15.4");
}

#[test]
fn pins_works_without_any_config() {
    let repo = build_repo();
    // no config flag and none discoverable from a tempdir cwd: the verb is a pure repo read
    let out = Command::cargo_bin("backhopper")
        .unwrap()
        .current_dir(repo.dir.path())
        .args([
            "--formatter",
            "text",
            "series",
            "pins",
            "--repo-dir-path",
            ".",
            "--branch",
            "old",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("cowboy\thex\t2.13.0"));
}

#[test]
fn pins_emits_json_payload() {
    let repo = build_repo();
    let out = run_fmt(&repo, "json", &["--branch", "old"])
        .success()
        .get_output()
        .clone();
    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).expect("valid json");
    assert_eq!(parsed["command"], "series pins");
    let pins = parsed["data"]["pins"].as_array().unwrap();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0]["name"], "cowboy");
    assert_eq!(pins[0]["source"], "hex");
    assert_eq!(pins[0]["version"], "2.13.0");
}

#[test]
fn pins_against_branch_reports_divergence() {
    let repo = build_repo();
    let out = run(&repo, &["--branch", "main", "--against-branch", "old"])
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("cowboy: hex 2.16.0 (main) vs hex 2.13.0 (old)"));
    assert!(stdout.contains("gun: git 2.2.0 only on main"));
    assert!(!stdout.contains("ra:"));
}

#[test]
fn pins_against_identical_branch_reports_no_divergence() {
    let repo = build_repo();
    let out = run(&repo, &["--branch", "main", "--against-branch", "main"])
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("no dep pin divergence"));
}

#[test]
fn pins_divergence_json_carries_all_three_buckets() {
    let repo = build_repo();
    let out = run_fmt(
        &repo,
        "json",
        &["--branch", "main", "--against-branch", "old"],
    )
    .success()
    .get_output()
    .clone();
    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).expect("valid json");
    let data = &parsed["data"];
    assert_eq!(data["diverged"][0]["name"], "cowboy");
    assert_eq!(data["diverged"][0]["branch_version"], "2.16.0");
    assert_eq!(data["diverged"][0]["against_version"], "2.13.0");
    assert_eq!(data["only_on_branch"][0]["name"], "gun");
    assert!(data["only_on_against"].as_array().unwrap().is_empty());
}

#[test]
fn pins_distinguishes_missing_branch_from_missing_file() {
    let repo = build_repo();
    let missing_branch = run(&repo, &["--branch", "no-such-branch"])
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8(missing_branch.stderr).unwrap();
    assert!(err.contains("could not resolve"), "got: {err}");

    let bare = GitRepoFixture::new();
    bare.write_file("README.md", "x");
    bare.commit("no components file");
    let missing_file = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "series",
            "pins",
            "--repo-dir-path",
            bare.dir.path().to_str().unwrap(),
            "--branch",
            "main",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8(missing_file.stderr).unwrap();
    assert!(err.contains("not found at"), "got: {err}");
}
