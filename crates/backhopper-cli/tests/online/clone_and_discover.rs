use core::time::Duration;
use std::path::{Path, PathBuf};
use std::process::Command as Std;
use std::str::FromStr;

use assert_cmd::Command;
use tempfile::TempDir;

use backhopper_core::model::names::{Mfa, ProjectName, TagName};
use backhopper_core::store::SnapshotStore;

const RA_REPO: &str = "https://github.com/rabbitmq/ra.git";
const KHEPRI_REPO: &str = "https://github.com/rabbitmq/khepri.git";

fn shallow_clone(url: &str, dest: &Path) {
    let status = Std::new("git")
        .args([
            "clone",
            "--depth",
            "50",
            "--no-tags",
            "--filter=blob:none",
            url,
            dest.to_str().expect("utf-8 path"),
        ])
        .status()
        .expect("git clone");
    assert!(status.success(), "git clone {} failed", url);
    let unshallow = Std::new("git")
        .args(["fetch", "--tags", "--depth=200"])
        .current_dir(dest)
        .status()
        .expect("git fetch tags");
    assert!(unshallow.success(), "git fetch tags failed for {}", url);
}

fn write_config(workdir: &Path, project: &str, repo: &Path, snapshot_dir: &Path) -> PathBuf {
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl", "include/**/*.hrl"]

[[project]]
name    = "{}"
git_url = "{}"
"#,
        snapshot_dir.display(),
        project,
        repo.display(),
    );
    let cfg = workdir.join("backhopper.toml");
    std::fs::write(&cfg, body).unwrap();
    cfg
}

#[test]
#[ignore = "online: clones rabbitmq/ra"]
fn ra_upstream_clone_then_discover() {
    let work = TempDir::new().unwrap();
    let repo = work.path().join("ra-clone");
    shallow_clone(RA_REPO, &repo);

    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), "ra", &repo, &snap);

    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .timeout(Duration::from_mins(2))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
            "--project",
            "ra",
            "--formatter",
            "text",
        ])
        .assert();
    assert.success();

    let any_snapshot = std::fs::read_dir(snap.join("ra"))
        .unwrap()
        .next()
        .expect("at least one snapshot captured");
    let path = any_snapshot.unwrap().path();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("module ra"), "snapshot lacks `module ra`");
}

#[test]
#[ignore = "online: clones rabbitmq/khepri"]
fn khepri_upstream_clone_then_lookup() {
    let work = TempDir::new().unwrap();
    let repo = work.path().join("khepri-clone");
    shallow_clone(KHEPRI_REPO, &repo);

    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), "khepri", &repo, &snap);

    Command::cargo_bin("backhopper")
        .unwrap()
        .timeout(Duration::from_mins(2))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
            "--project",
            "khepri",
            "--formatter",
            "text",
        ])
        .assert()
        .success();

    let store = SnapshotStore::open(&snap).unwrap();
    let project = ProjectName::new("khepri").unwrap();
    let tags = store.list_tags(&project).unwrap();
    assert!(
        !tags.is_empty(),
        "expected at least one captured khepri tag"
    );
    let recent = TagName::new("v0.18.0").unwrap();
    if tags.iter().any(|t| t == &recent) {
        let s = store.read(&project, &recent).unwrap();
        let mfa = Mfa::from_str("khepri:get/1").unwrap();
        let _ = s.lookup_export(&mfa.module, &mfa.function, mfa.arity);
    }
}
