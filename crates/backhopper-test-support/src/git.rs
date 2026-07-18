// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A temp git repository built by shelling out to the `git` binary.
//! Test helpers may shell out; production code uses `gix`.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

#[derive(Debug)]
pub struct GitRepoFixture {
    pub dir: TempDir,
}

impl GitRepoFixture {
    pub fn new() -> Self {
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

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn write_file(&self, rel: &str, body: &str) {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        run(self.dir.path(), &["git", "add", rel]);
    }

    /// Stage everything, deletions included. `write_file` only adds
    /// the written path, so tests that remove files call this.
    pub fn stage_all(&self) {
        run(self.dir.path(), &["git", "add", "-A"]);
    }

    pub fn commit(&self, message: &str) {
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

    /// Commit with explicit author and committer dates, for tests
    /// that exercise time-bounded walks.
    pub fn commit_dated(&self, message: &str, date: &str) {
        run_with_dates(
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
            date,
        );
    }

    /// `git cherry-pick -x <sha>`: records the source SHA trailer the
    /// suppression filter keys on.
    pub fn cherry_pick_x(&self, sha: &str) {
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

    // attempt the real cherry-pick, record whether it conflicted, then restore the branch either way
    pub fn cherry_pick_conflicts(&self, sha: &str) -> bool {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "cherry-pick",
                sha,
            ])
            .current_dir(self.dir.path())
            .status()
            .expect("git cherry-pick");
        if status.success() {
            run(self.dir.path(), &["git", "reset", "--hard", "-q", "HEAD~1"]);
            false
        } else {
            run(self.dir.path(), &["git", "cherry-pick", "--abort"]);
            true
        }
    }

    pub fn tag(&self, name: &str) {
        run(self.dir.path(), &["git", "tag", name]);
    }

    pub fn annotated_tag(&self, name: &str, message: &str) {
        run(
            self.dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "tag",
                "-a",
                name,
                "-m",
                message,
            ],
        );
    }

    pub fn checkout_new_branch(&self, name: &str) {
        run(self.dir.path(), &["git", "checkout", "-q", "-b", name]);
    }

    pub fn checkout(&self, name: &str) {
        run(self.dir.path(), &["git", "checkout", "-q", name]);
    }

    /// Merge with `--no-ff` so a 2-parent merge object always exists.
    pub fn merge_no_ff(&self, branch: &str, message: &str) {
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

    /// Merge that keeps the current tree (`-s ours`): a 2-parent merge
    /// whose first-parent diff is empty.
    pub fn merge_ours(&self, branch: &str, message: &str) {
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

    /// Octopus merge of two branches: a 3-parent commit.
    pub fn merge_octopus(&self, first: &str, second: &str, message: &str) {
        run(
            self.dir.path(),
            &[
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "merge",
                "-m",
                message,
                first,
                second,
            ],
        );
    }

    pub fn head_sha(&self) -> String {
        rev_parse_output(self.dir.path(), "HEAD")
    }

    pub fn rev_parse(&self, spec: &str) -> String {
        rev_parse_output(self.dir.path(), spec)
    }

    /// The SHA of the repository's root (parentless) commit.
    pub fn root_sha(&self) -> String {
        let output = Command::new("git")
            .args(["rev-list", "--max-parents=0", "HEAD"])
            .current_dir(self.dir.path())
            .output()
            .expect("git rev-list");
        assert!(output.status.success(), "rev-list root failed");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim()
            .to_owned()
    }
}

impl Default for GitRepoFixture {
    fn default() -> Self {
        Self::new()
    }
}

fn run(cwd: &Path, argv: &[&str]) {
    let status = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .status()
        .expect("git");
    assert!(status.success(), "{argv:?} failed");
}

fn run_with_dates(cwd: &Path, argv: &[&str], date: &str) {
    let status = Command::new(argv[0])
        .args(&argv[1..])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(cwd)
        .status()
        .expect("git");
    assert!(status.success(), "{argv:?} failed");
}

fn rev_parse_output(cwd: &Path, spec: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", spec])
        .current_dir(cwd)
        .output()
        .expect("git rev-parse");
    assert!(output.status.success(), "rev-parse {spec} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
