use std::io::Write;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_TRACKED: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

fn build_repo() -> (FixtureRepo, TempDir) {
    let work = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_TRACKED);
    repo.commit("first");
    repo.tag("v1.0.0");
    (repo, work)
}

#[test]
fn otp_only_patch_is_compatible_with_zero_tracked_refs() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,3 @@
 -module(other).
+a(L) -> lists:foreach(fun(_) -> ok end, L).
+b(M) -> maps:get(k, M, default).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let default_output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let default_stdout = String::from_utf8(default_output.stdout).unwrap();
    assert!(
        default_stdout.contains("compatible: 1"),
        "got {}",
        default_stdout
    );
    assert!(
        default_stdout.contains("0 tracked symbols referenced"),
        "got {}",
        default_stdout
    );
    assert!(
        !default_stdout.contains("Untracked module calls"),
        "diagnostic section should be hidden by default: {}",
        default_stdout
    );

    let opt_in_output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--show-untracked-calls",
            "--show-otp-calls",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let opt_in_stdout = String::from_utf8(opt_in_output.stdout).unwrap();
    assert!(
        opt_in_stdout.contains("Untracked module calls"),
        "got {}",
        opt_in_stdout
    );
    assert!(opt_in_stdout.contains("lists"), "got {}", opt_in_stdout);
    assert!(opt_in_stdout.contains("maps"), "got {}", opt_in_stdout);
    assert!(
        opt_in_stdout.contains("OTP stdlib"),
        "got {}",
        opt_in_stdout
    );
}

#[test]
fn show_untracked_calls_without_show_otp_calls_hides_otp_only_entries() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,3 @@
 -module(other).
+a(L) -> lists:foreach(fun(_) -> ok end, L).
+b() -> ranch:info(my_ref).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--show-untracked-calls",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Untracked module calls"), "got {}", stdout);
    assert!(stdout.contains("ranch"), "got {}", stdout);
    assert!(stdout.contains("untracked project"), "got {}", stdout);
    assert!(
        !stdout.contains("OTP stdlib"),
        "OTP entries should be hidden without --show-otp-calls: {}",
        stdout
    );
}

#[test]
fn show_otp_calls_alone_implies_show_untracked_calls() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,2 @@
 -module(other).
+a(L) -> lists:foreach(fun(_) -> ok end, L).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let stdout = String::from_utf8(
        Command::cargo_bin("backhopper")
            .unwrap()
            .args([
                "--config",
                cfg.to_str().unwrap(),
                "--formatter",
                "text",
                "compatibility",
                "patch",
                "--project",
                "demo",
                "--tag",
                "v1.0.0",
                "--show-otp-calls",
                pf.path().to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(stdout.contains("Untracked module calls"), "got {}", stdout);
    assert!(stdout.contains("OTP stdlib"), "got {}", stdout);
}

#[test]
fn record_defined_in_untracked_project_does_not_flag_incompatible() {
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
    let patch_body = "\
diff --git a/deps/rabbit/src/rabbit_amqp_reader.erl b/deps/rabbit/src/rabbit_amqp_reader.erl
--- a/deps/rabbit/src/rabbit_amqp_reader.erl
+++ b/deps/rabbit/src/rabbit_amqp_reader.erl
@@ -1,1 +1,2 @@
 -module(rabbit_amqp_reader).
+go(#v1{user = #user{id = U}}) -> U.
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--show-untracked-calls",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("compatible: 1"), "got {}", stdout);
    assert!(
        !stdout.contains("MissingSymbol"),
        "rabbit-internal record should not flag MissingSymbol: {}",
        stdout
    );
    assert!(stdout.contains("Untracked records"), "got {}", stdout);
    assert!(stdout.contains("#user"), "got {}", stdout);
    assert!(stdout.contains("#v1"), "got {}", stdout);
}

#[test]
fn patch_touching_another_projects_deps_path_is_ignored() {
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
    let patch_body = "\
diff --git a/deps/rabbit/src/rabbit_misc.erl b/deps/rabbit/src/rabbit_misc.erl
--- a/deps/rabbit/src/rabbit_misc.erl
+++ b/deps/rabbit/src/rabbit_misc.erl
@@ -1,1 +1,2 @@
 -module(rabbit_misc).
+helper() -> rabbit_misc:r(<<\"q\">>, exchange, <<\"x\">>).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("compatible: 1"), "got {}", stdout);
    assert!(
        !stdout.contains("FileAbsent"),
        "FileAbsent leaked into verdict: {}",
        stdout
    );
}

#[test]
fn json_output_carries_tracked_refs_and_diagnostics() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,2 @@
 -module(other).
+a(L) -> lists:foreach(fun(_) -> ok end, L).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "json",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["exit_code"], 0);
    let pin = &parsed["data"]["results"]["results"][0];
    assert_eq!(pin["tracked_refs"], 0);
    assert_eq!(pin["verdict"]["verdict"], "compatible");
    assert_eq!(parsed["data"]["diagnostics"]["untracked_calls"]["lists"], 1);
}

#[test]
fn dynamic_dispatch_shows_under_show_untracked_calls() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,4 @@
 -module(other).
+init(#{module := Mod, args := Args}) ->
+    Pid = spawn_link(Mod, init, [Args]),
+    Mod:register(self()).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let stdout = String::from_utf8(
        Command::cargo_bin("backhopper")
            .unwrap()
            .args([
                "--config",
                cfg.to_str().unwrap(),
                "--formatter",
                "text",
                "compatibility",
                "patch",
                "--project",
                "demo",
                "--tag",
                "v1.0.0",
                "--show-untracked-calls",
                pf.path().to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(stdout.contains("compatible: 1"), "got {}", stdout);
    assert!(
        stdout.contains("Unanalyzed dynamic calls"),
        "got {}",
        stdout
    );
    assert!(stdout.contains("apply-family BIFs"), "got {}", stdout);
    assert!(stdout.contains("variable-dispatch calls"), "got {}", stdout);
}

#[test]
fn unanalyzed_block_hidden_without_show_untracked_calls() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,3 @@
 -module(other).
+go(Mod, Args) ->
+    apply(Mod, init, [Args]).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let stdout = String::from_utf8(
        Command::cargo_bin("backhopper")
            .unwrap()
            .args([
                "--config",
                cfg.to_str().unwrap(),
                "--formatter",
                "text",
                "compatibility",
                "patch",
                "--project",
                "demo",
                "--tag",
                "v1.0.0",
                pf.path().to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        !stdout.contains("Unanalyzed dynamic calls"),
        "default text mode must not show the unanalyzed block: {}",
        stdout
    );
}

#[test]
fn json_output_carries_unanalyzed_counts() {
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
    let patch_body = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,3 @@
 -module(other).
+go(Mod, Args) ->
+    apply(Mod, init, [Args]).
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(patch_body.as_bytes()).unwrap();
    let output = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--formatter",
            "json",
            "compatibility",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let unanalyzed = &parsed["data"]["diagnostics"]["unanalyzed"];
    assert_eq!(unanalyzed["apply"], 1);
}
