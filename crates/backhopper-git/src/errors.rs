// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fmt::Display;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Wrap any `gix` error as a `GitError::Gix`, the one conversion the
/// crate's `.map_err` sites share.
pub(crate) fn gix_err<E: Display>(e: E) -> GitError {
    GitError::Gix(e.to_string())
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("repository open failed: {0}")]
    OpenFailed(String),

    #[error("not a git repository: {0:?}")]
    NotAGitRepository(PathBuf),

    #[error("tag {0:?} not found")]
    TagNotFound(String),

    #[error("commit {0:?} not found")]
    CommitNotFound(String),

    #[error(
        "commit sha prefix {prefix:?} matched {truncated_at} object{}",
        if *truncated_at == 1 { "" } else { "s" }
    )]
    AmbiguousSha {
        prefix: String,
        candidates: Vec<String>,
        truncated_at: u32,
    },

    #[error("commit sha prefix {prefix:?} resolved to {kind}, not a commit")]
    NotACommit {
        prefix: String,
        kind: String,
        resolved: String,
    },

    #[error("path {path:?} not present at commit {commit}")]
    PathNotPresent { commit: String, path: String },

    #[error("gix error: {0}")]
    Gix(String),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}
