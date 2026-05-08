use assert_cmd::Command;
use std::io::Write;
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

#[test]
fn compat_patch_flags_missing_function_as_incompatible() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
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
            "--config",
            cfg.to_str().unwrap(),
            "compatibility",
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
    let output = assert.code(1).get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("incompatible: 1"), "got {}", stdout);
}

#[test]
fn compat_patch_compatible_when_only_existing_calls() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
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
            "--config",
            cfg.to_str().unwrap(),
            "compatibility",
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
        .success();
}
