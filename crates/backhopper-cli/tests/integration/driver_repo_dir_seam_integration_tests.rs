// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The driver assembles per-verb global flags the real CLI must accept.
//! `--repo-dir-path` is defined per verb, so a repo-free verb rejects it:
//! the driver must gate it, and this exercises the whole seam against the
//! real binary rather than a mock.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use backhopper_driver::{Backhopper, Verb};
use predicates::str::contains;
use tempfile::TempDir;

#[test]
fn version_succeeds_through_the_driver_with_a_repo_dir_configured() {
    let repo = TempDir::new().unwrap();
    let mut driver = Backhopper::with_binary_path(cargo_bin("backhopper"));
    driver.options_mut().repo_dir_path = Some(repo.path().to_path_buf());
    let info = driver
        .version()
        .expect("version must not receive --repo-dir-path");
    assert!(!info.version.is_empty());
}

#[test]
fn cli_rejects_repo_dir_path_on_a_repo_free_verb() {
    let repo = TempDir::new().unwrap();
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(["version", "--repo-dir-path"])
        .arg(repo.path())
        .assert()
        .failure()
        .stderr(contains("unexpected argument '--repo-dir-path'"));
}

#[test]
fn cli_accepts_repo_dir_path_on_verbs_the_driver_forwards_it_to() {
    for verb in Verb::iter().filter(|v| v.accepts_global_repo_dir()) {
        Command::cargo_bin("backhopper")
            .unwrap()
            .args(verb.cli_path())
            .arg("--repo-dir-path")
            .arg(".")
            .arg("--help")
            .assert()
            .success();
    }
}
