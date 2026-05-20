// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("snapshot error: {0}")]
    Snapshot(#[from] SnapshotError),

    #[error("store error: {0}")]
    Store(#[from] StoreError),

    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("git error: {0}")]
    Git(#[from] GitError),

    #[error("patch error: {0}")]
    Patch(#[from] PatchError),

    #[error("name error: {0}")]
    Name(#[from] NameError),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum NameError {
    #[error("empty value where {kind} was expected")]
    Empty { kind: &'static str },

    #[error("{kind} too long: {len} > {max}")]
    TooLong {
        kind: &'static str,
        len: usize,
        max: usize,
    },

    #[error("invalid character {ch:?} in {kind}: {value}")]
    InvalidCharacter {
        kind: &'static str,
        ch: char,
        value: String,
    },

    #[error("{kind} {value:?} does not match required pattern {pattern}")]
    PatternMismatch {
        kind: &'static str,
        value: String,
        pattern: &'static str,
    },

    #[error("invalid commit sha {value:?}: must be exactly 40 lowercase hex characters")]
    InvalidCommitSha { value: String },

    #[error("invalid arity: {raw:?} could not be parsed as an integer in 0..=255")]
    InvalidArityParse { raw: String },

    #[error("invalid arity {value}: must be 0..=255")]
    InvalidArity { value: i64 },

    #[error("invalid mfa {value:?}: expected 'mod:fun/arity'")]
    InvalidMfa { value: String },
}

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("malformed header at line {line}: {detail}")]
    MalformedHeader { line: usize, detail: String },

    #[error("unknown header key {key:?} at line {line}")]
    UnknownHeaderKey { line: usize, key: String },

    #[error("missing required header key {key:?}")]
    MissingHeaderKey { key: &'static str },

    #[error("unknown format-version {found}: expected {expected}")]
    UnknownFormatVersion { found: String, expected: u32 },

    #[error("unexpected token at line {line}: {detail}")]
    UnexpectedToken { line: usize, detail: String },

    #[error("non-canonical input at line {line}: {detail}")]
    NotCanonical { line: usize, detail: String },

    #[error("invalid utf-8 at byte offset {offset}")]
    InvalidUtf8 { offset: usize },

    #[error("input exceeds size limit ({size} > {limit} bytes)")]
    SizeLimit { size: usize, limit: usize },

    #[error("name error: {0}")]
    Name(#[from] NameError),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("snapshot store root does not exist: {0}")]
    RootMissing(PathBuf),

    #[error("snapshot not found: project {project}, tag {tag}")]
    SnapshotNotFound { project: String, tag: String },

    #[error("snapshot already exists: {0}")]
    AlreadyExists(PathBuf),

    #[error("path escape: {0:?}")]
    PathEscape(PathBuf),

    #[error("snapshot error: {0}")]
    Snapshot(#[from] SnapshotError),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),

    #[error("malformed toml: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("unknown config-version {0}")]
    UnknownConfigVersion(u32),

    #[error("project {0:?} referenced but not configured")]
    UnknownProject(String),

    #[error("series {0:?} referenced but not configured")]
    UnknownSeries(String),

    #[error("series {series:?} pins unknown project {project:?}")]
    SeriesPinsUnknownProject { series: String, project: String },

    #[error("name error: {0}")]
    Name(#[from] NameError),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("repository open failed: {0}")]
    OpenFailed(String),

    #[error("tag {0:?} not found")]
    TagNotFound(String),

    #[error("commit {0:?} not found")]
    CommitNotFound(String),

    #[error("path {path:?} not present at commit {commit}")]
    PathNotPresent { commit: String, path: String },

    #[error("gix error: {0}")]
    Gix(String),

    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("malformed unified diff at line {line}: {detail}")]
    Malformed { line: usize, detail: String },

    #[error("input exceeds size limit ({size} > {limit} bytes)")]
    SizeLimit { size: usize, limit: usize },

    #[error("patch is not valid UTF-8 (first invalid byte at offset {offset})")]
    InvalidUtf8 { offset: usize },

    #[error("unsupported file type: {0}")]
    UnsupportedFileType(String),

    #[error("binary patch is not analyzable")]
    BinaryPatch,
}
