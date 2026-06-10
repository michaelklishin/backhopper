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

    #[error("commit sha prefix is empty")]
    EmptyCommitShaPrefix,

    #[error("invalid commit sha prefix {value:?}: too short ({len} chars; needs 7 to 40)")]
    CommitShaPrefixTooShort { value: String, len: usize },

    #[error("invalid commit sha prefix {value:?}: too long ({len} chars; needs 7 to 40)")]
    CommitShaPrefixTooLong { value: String, len: usize },

    #[error("invalid commit sha prefix {value:?}: non-hex character {ch:?} at position {position}")]
    CommitShaPrefixNonHex {
        value: String,
        ch: char,
        position: usize,
    },

    #[error("invalid arity: {raw:?} could not be parsed as an integer in 0..=255")]
    InvalidArityParse { raw: String },

    #[error("invalid arity {value}: must be 0..=255")]
    InvalidArity { value: i64 },

    #[error("invalid mfa {value:?}: expected 'mod:fun/arity'")]
    InvalidMfa { value: String },

    #[error("invalid {kind} {value:?}: {reason}")]
    Invalid {
        kind: &'static str,
        value: String,
        reason: &'static str,
    },
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

    #[error("pin pattern {pattern:?} for project {project:?} matched no snapshot tag")]
    PinPatternNoMatch { project: String, pattern: String },

    #[error("snapshot for pin {project} @ {tag} is not on disk at {expected_path}")]
    PinSnapshotMissing {
        project: String,
        tag: String,
        expected_path: String,
    },

    #[error(
        "pin pattern {pattern:?} for project {project:?} could not be resolved: \
         snapshot store error: {detail}"
    )]
    PinPatternStore {
        project: String,
        pattern: String,
        detail: String,
    },

    #[error("pin `select` must be \"latest\" or \"oldest\", got {0:?}")]
    PinUnknownSelect(String),

    #[error("unknown project layout {0:?}: expected single_app, multi_app, or erlang_otp")]
    UnknownProjectLayout(String),

    #[error("unknown project kind {0:?}: expected external or self")]
    UnknownProjectKind(String),

    #[error(
        "unknown project family {0:?}: expected generic, erlang_otp, ra, osiris, khepri, or rabbitmq"
    )]
    UnknownProjectFamily(String),

    #[error("project {0:?} has kind=\"self\" but also specifies git_url; remove one")]
    SelfProjectHasGitUrl(String),

    #[error("project {0:?} requires git_url (external kind, the default)")]
    ExternalProjectMissingGitUrl(String),

    #[error("config defines more than one project with kind=\"self\": {projects:?}")]
    MultipleSelfProjects { projects: Vec<String> },

    #[error("self-project pin {project} @ {git_ref} requires `--repo-dir-path` for resolution")]
    SelfPinNeedsRepoDirPath { project: String, git_ref: String },

    #[error(
        "series pins {project} as a self-pin (branch/sha form) but {project} is configured as kind=\"external\""
    )]
    SelfPinReferencesExternalProject { project: String },

    #[error("self-project {0:?} has no git_url; operations needing one must use `--repo-dir-path`")]
    SelfProjectHasNoGitUrl(String),

    #[error("project {project:?} has layout {layout:?} but no `app_roots` defaulted or set")]
    LayoutWithoutAppRoots { project: String, layout: String },

    #[error("suite_rule[{rule_index}]: invalid regex: {detail}")]
    SuiteRuleRegex { rule_index: usize, detail: String },

    #[error("suite_rule[{rule_index}]: template references unknown capture group {placeholder:?}")]
    SuiteRuleUnknownPlaceholder {
        rule_index: usize,
        placeholder: String,
    },

    #[error(
        "suite_rule[{rule_index}]: include_suite_for_dep_modules requires a named capture `dep` from the path or line regex"
    )]
    SuiteRuleMissingDepCapture { rule_index: usize },

    #[error("path_translation file not found: {0:?}")]
    PathTranslationFileMissing(PathBuf),

    #[error("path_translation: name must be non-empty")]
    PathTranslationEmptyName,

    #[error("path_translation[{name:?}]: {field} must be non-empty")]
    PathTranslationEmptyPrefix { name: String, field: &'static str },

    #[error("path_translation[{name:?}]: {field} {value:?} must end in '/'")]
    PathTranslationPrefixNoTrailingSlash {
        name: String,
        field: &'static str,
        value: String,
    },

    #[error("path_translation[{name:?}]: unknown direction {direction:?}")]
    PathTranslationUnknownDirection { name: String, direction: String },

    #[error("path_translation: duplicate name {0:?}")]
    PathTranslationDuplicateName(String),

    #[error("path_translation: duplicate source_prefix {0:?}")]
    PathTranslationDuplicateSource(String),

    #[error(
        "path_translation: source_prefix overlap: {a:?} and {b:?} (one is a strict prefix of the other)"
    )]
    PathTranslationPrefixOverlap { a: String, b: String },

    #[error("name error: {0}")]
    Name(#[from] NameError),

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
