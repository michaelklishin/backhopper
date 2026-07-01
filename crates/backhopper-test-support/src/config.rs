// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A minimal `backhopper.toml` for a single `demo` project, written
//! into a test working directory.

use std::path::{Path, PathBuf};

/// Render `path` for embedding inside a TOML basic string. `Path::display`
/// yields `\`-separated paths on Windows, and TOML basic strings treat `\`
/// as an escape introducer, so an unescaped path corrupts the parse.
pub fn toml_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}

pub fn write_config(dir: &Path, repo_path: &Path, snapshot_dir: &Path) -> PathBuf {
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
        toml_path(snapshot_dir),
        toml_path(repo_path),
    );
    let cfg = dir.join("backhopper.toml");
    std::fs::write(&cfg, body).unwrap();
    cfg
}
