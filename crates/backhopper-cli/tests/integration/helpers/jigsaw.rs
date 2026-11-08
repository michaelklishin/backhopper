// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use backhopper_test_support::GitRepoFixture;

use super::cli::{run, stdout};

pub(crate) const PARTIAL_SUCCESS: i32 = 3;

pub(crate) struct Workspace {
    pub(crate) dir: TempDir,
    pub(crate) config: PathBuf,
    pub(crate) repos: Vec<GitRepoFixture>,
}

impl Workspace {
    pub(crate) fn new(repos: Vec<GitRepoFixture>, config: impl FnOnce(&Path) -> String) -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backhopper.toml");
        fs::write(&path, config(dir.path())).unwrap();
        Self {
            dir,
            config: path,
            repos,
        }
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub(crate) fn json(&self, args: &[&str]) -> (i32, Value) {
        let mut full = vec![
            "--config-file-path",
            self.config.to_str().unwrap(),
            "--formatter",
            "json",
        ];
        full.extend_from_slice(args);
        let assert = run(full);
        let output = assert.get_output().clone();
        let code = output.status.code().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        (code, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub(crate) fn text(&self, args: &[&str]) -> (i32, String) {
        let mut full = vec!["--config-file-path", self.config.to_str().unwrap()];
        full.extend_from_slice(args);
        let assert = run(full);
        (assert.get_output().status.code().unwrap(), stdout(&assert))
    }

    pub(crate) fn remove_repositories(&self) {
        for repo in &self.repos {
            fs::remove_dir_all(repo.path()).unwrap();
        }
    }

    /// Returns the plan file.
    pub(crate) fn stock(&self) -> PathBuf {
        let plan = self.path("plan.json");
        let plan_path = plan.to_str().unwrap();
        let (code, _) = self.json(&[
            "jigsaw",
            "plan",
            "--host",
            "queue",
            "--series",
            "1.0",
            "--plan-file-path",
            plan_path,
        ]);
        assert_eq!(code, PARTIAL_SUCCESS);
        let (code, generated) =
            self.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
        assert_eq!(code, 0, "{generated}");
        plan
    }
}
