// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::io::Write;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::{NamedTempFile, TempDir};

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_OLD: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

const PATCH_REFERENCING_MISSING: &str = "\
diff --git a/src/demo_mod.erl b/src/demo_mod.erl
--- a/src/demo_mod.erl
+++ b/src/demo_mod.erl
@@ -1,3 +1,4 @@
 -module(demo_mod).
 -export([greet/1]).
+greet(N, Extra) -> demo_mod:does_not_exist(N, Extra).
 greet(Name) -> Name.
";

fn build_repo() -> (FixtureRepo, TempDir) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_OLD);
    repo.commit("first");
    repo.tag("v1.0.0");
    (repo, workdir)
}

fn generate_snapshot(cfg: &Path) {
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success();
}

fn run_check_patch(cfg: &Path, patch_path: &Path) -> assert_cmd::assert::Assert {
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--formatter",
            "text",
            patch_path.to_str().unwrap(),
        ])
        .assert()
}

fn write_patch_to_temp(body: &str) -> NamedTempFile {
    let mut patch_file = NamedTempFile::new().unwrap();
    patch_file.write_all(body.as_bytes()).unwrap();
    patch_file
}

fn write_config_with(
    dir: &Path,
    repo_path: &Path,
    snapshot_dir: &Path,
    extra_lines: &str,
) -> PathBuf {
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl", "include/**/*.hrl"]

[[project]]
name    = "demo"
git_url = "{}"
{}
"#,
        snapshot_dir.display(),
        repo_path.display(),
        extra_lines,
    );
    let cfg = dir.join("backhopper.toml");
    std::fs::write(&cfg, body).unwrap();
    cfg
}

#[test]
fn compat_patch_flags_missing_function_as_incompatible() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let mut patch_file = NamedTempFile::new().unwrap();
    patch_file
        .write_all(PATCH_REFERENCING_MISSING.as_bytes())
        .unwrap();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--formatter",
            "text",
            patch_file.path().to_str().unwrap(),
        ])
        .assert();
    let output = assert.code(3).get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("incompatible: 1"), "got {stdout}");
}

#[test]
fn compat_patch_compatible_when_only_existing_calls() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let body = "\
diff --git a/src/demo_mod.erl b/src/demo_mod.erl
--- a/src/demo_mod.erl
+++ b/src/demo_mod.erl
@@ -1,3 +1,4 @@
 -module(demo_mod).
 -export([greet/1]).
+example() -> demo_mod:greet(<<\"x\">>).
 greet(Name) -> Name.
";
    let mut patch_file = NamedTempFile::new().unwrap();
    patch_file.write_all(body.as_bytes()).unwrap();
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--formatter",
            "text",
            patch_file.path().to_str().unwrap(),
        ])
        .assert()
        // Preimage drift on hunk #0: RequiresAdaptation, exit 3.
        .code(3);
}

const PATCH_REFERENCING_WRONG_ARITY: &str = "\
diff --git a/src/demo_mod.erl b/src/demo_mod.erl
--- a/src/demo_mod.erl
+++ b/src/demo_mod.erl
@@ -1,3 +1,4 @@
 -module(demo_mod).
 -export([greet/1]).
+caller() -> demo_mod:greet(a, b, c).
 greet(Name) -> Name.
";

#[test]
fn compat_patch_flags_arity_change_as_incompatible() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    generate_snapshot(&cfg);
    let patch_file = write_patch_to_temp(PATCH_REFERENCING_WRONG_ARITY);
    let assert = run_check_patch(&cfg, patch_file.path());
    let output = assert.code(3).get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("incompatible: 1"),
        "expected incompatible verdict, got: {stdout}"
    );
    assert!(
        stdout.contains("ArityChanged"),
        "expected ArityChanged reason, got: {stdout}"
    );
}

const ERL_INTERNAL_MODULE: &str = r#"
-module(internal_mod).
-export([secret/0]).
secret() -> ok.
"#;

const PATCH_REFERENCING_HIDDEN: &str = "\
diff --git a/src/demo_mod.erl b/src/demo_mod.erl
--- a/src/demo_mod.erl
+++ b/src/demo_mod.erl
@@ -1,3 +1,4 @@
 -module(demo_mod).
 -export([greet/1]).
+caller() -> internal_mod:secret().
 greet(Name) -> Name.
";

#[test]
fn compat_patch_flags_now_hidden_module() {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_OLD);
    repo.write_file("src/internal_mod.erl", ERL_INTERNAL_MODULE);
    repo.commit("first");
    repo.tag("v1.0.0");
    let snap = workdir.path().join("snapshots");
    let cfg = write_config_with(
        workdir.path(),
        repo.dir.path(),
        &snap,
        r#"internal_modules = ["internal_mod"]"#,
    );
    generate_snapshot(&cfg);
    let patch_file = write_patch_to_temp(PATCH_REFERENCING_HIDDEN);
    let assert = run_check_patch(&cfg, patch_file.path());
    let output = assert.code(3).get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("incompatible: 1") || stdout.contains("requires_adaptation: 1"),
        "expected non-Compatible verdict, got: {stdout}"
    );
    assert!(
        stdout.contains("NowHidden"),
        "expected NowHidden reason, got: {stdout}"
    );
}

const ERL_MULTI_HUNK: &str = r#"-module(demo_mod).
-export([greet/1, a/0, b/0, c/0, d/0]).

a() -> ok.

b() -> ok.

c() -> ok.

d() -> ok.

greet(Name) -> Name.
"#;

const PATCH_DRIFTS_AT_SECOND_HUNK: &str = concat!(
    "diff --git a/src/demo_mod.erl b/src/demo_mod.erl\n",
    "--- a/src/demo_mod.erl\n",
    "+++ b/src/demo_mod.erl\n",
    "@@ -3,3 +3,4 @@\n",
    "\n",
    " a() -> ok.\n",
    "\n",
    "+new_a_helper() -> ok.\n",
    "@@ -9,3 +10,4 @@\n",
    " wrong_function_text_one\n",
    " wrong_function_text_two\n",
    "+new_b_helper() -> ok.\n",
    " wrong_function_text_three\n",
);

#[test]
fn compat_patch_detects_drift_at_non_zero_hunk_index() {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_MULTI_HUNK);
    repo.commit("first");
    repo.tag("v1.0.0");
    let snap = workdir.path().join("snapshots");
    let cfg = write_config(workdir.path(), repo.dir.path(), &snap);
    generate_snapshot(&cfg);
    let patch_file = write_patch_to_temp(PATCH_DRIFTS_AT_SECOND_HUNK);
    let assert = run_check_patch(&cfg, patch_file.path());
    let output = assert.get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("PreimageMissing"),
        "expected PreimageMissing reason, got: {stdout}"
    );
    assert!(
        stdout.contains("hunk #1"),
        "expected the second hunk (zero-indexed) flagged, got: {stdout}"
    );
    assert!(
        !stdout.contains("hunk #0"),
        "expected hunk #0 to be clean, got: {stdout}"
    );
}

const PATCH_REFERENCING_UNTRACKED: &str = "\
diff --git a/src/demo_mod.erl b/src/demo_mod.erl
--- a/src/demo_mod.erl
+++ b/src/demo_mod.erl
@@ -1,3 +1,4 @@
 -module(demo_mod).
 -export([greet/1]).
+caller() -> amqp_utils:foo().
 greet(Name) -> Name.
";

#[test]
fn resolve_untracked_modules_flips_verdict_when_file_absent_in_repo() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    generate_snapshot(&cfg);
    let target_checkout = TempDir::new().unwrap();
    std::fs::create_dir_all(target_checkout.path().join("src")).unwrap();
    std::fs::write(
        target_checkout.path().join("src/demo_mod.erl"),
        "-module(demo_mod).\n",
    )
    .unwrap();
    let patch_file = write_patch_to_temp(PATCH_REFERENCING_UNTRACKED);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo-dir-path",
            target_checkout.path().to_str().unwrap(),
            "--resolve-untracked-modules",
            "--formatter",
            "text",
            patch_file.path().to_str().unwrap(),
        ])
        .assert();
    let output = assert.code(3).get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("UntrackedModuleMissing"),
        "expected UntrackedModuleMissing reason, got: {stdout}"
    );
    assert!(
        stdout.contains("amqp_utils"),
        "expected amqp_utils to be flagged, got: {stdout}"
    );
}

#[test]
fn resolve_untracked_modules_stays_compatible_when_file_present() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    generate_snapshot(&cfg);
    let target_checkout = TempDir::new().unwrap();
    std::fs::create_dir_all(target_checkout.path().join("src")).unwrap();
    std::fs::write(
        target_checkout.path().join("src/demo_mod.erl"),
        "-module(demo_mod).\n",
    )
    .unwrap();
    std::fs::write(
        target_checkout.path().join("src/amqp_utils.erl"),
        "-module(amqp_utils).\n-export([foo/0]).\nfoo() -> ok.\n",
    )
    .unwrap();
    let patch_file = write_patch_to_temp(PATCH_REFERENCING_UNTRACKED);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo-dir-path",
            target_checkout.path().to_str().unwrap(),
            "--resolve-untracked-modules",
            "--formatter",
            "text",
            patch_file.path().to_str().unwrap(),
        ])
        .assert()
        // Patch context drift produces RequiresAdaptation (exit 3): the test
        // asserts the resolver did not escalate to Incompatible.
        .code(3);
}
