// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

pub(crate) struct FixtureRepo {
    pub(crate) dir: TempDir,
}

impl FixtureRepo {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        run(dir.path(), &["git", "init", "-q", "-b", "main"]);
        run(
            dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        Self { dir }
    }

    pub(crate) fn write_file(&self, rel: &str, body: &str) {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        run(self.dir.path(), &["git", "add", rel]);
    }

    pub(crate) fn commit(&self, message: &str) {
        run(
            self.dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "commit",
                "-m",
                message,
            ],
        );
    }

    pub(crate) fn tag(&self, name: &str) {
        run(self.dir.path(), &["git", "tag", name]);
    }

    // Only consumed by the unix-gated cross-branch tests; gate to
    // match so Windows CI doesn't flag dead code.
    #[cfg(unix)]
    pub(crate) fn cherry_pick_x(&self, sha: &str) {
        run(
            self.dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "cherry-pick",
                "-x",
                sha,
            ],
        );
    }

    // Only consumed by `end_to_end_target_repo_integration_tests`, which is
    // `#![cfg(unix)]`. Gate to match so Windows CI doesn't flag dead code.
    #[cfg(unix)]
    pub(crate) fn checkout_new_branch(&self, name: &str) {
        run(self.dir.path(), &["git", "checkout", "-q", "-b", name]);
    }

    pub(crate) fn checkout(&self, name: &str) {
        run(self.dir.path(), &["git", "checkout", "-q", name]);
    }

    pub(crate) fn merge_ours(&self, branch: &str, message: &str) {
        run(
            self.dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "merge",
                "-s",
                "ours",
                "-m",
                message,
                branch,
            ],
        );
    }

    pub(crate) fn merge_no_ff(&self, branch: &str, message: &str) {
        run(
            self.dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "merge",
                "--no-ff",
                "-m",
                message,
                branch,
            ],
        );
    }

    pub(crate) fn head_sha(&self) -> String {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(self.dir.path())
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }
}

pub(crate) fn write_config(dir: &Path, repo_path: &Path, snapshot_dir: &Path) -> PathBuf {
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
"#,
        snapshot_dir.display(),
        repo_path.display(),
    );
    let cfg = dir.join("backhopper.toml");
    std::fs::write(&cfg, body).unwrap();
    cfg
}

fn run(cwd: &Path, argv: &[&str]) {
    let status = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .status()
        .expect("git");
    assert!(status.success(), "{argv:?} failed");
}
