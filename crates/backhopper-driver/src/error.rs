// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Errors produced by the driver.
//!
//! The model splits five distinct failure modes apart:
//! binary-not-found, spawn, timeout, cancellation, tool error
//! (CLI exited with a non-success-shaped code), schema-version
//! mismatch, unparseable output, output-too-large, and
//! transport-level errors from custom backends.

use std::error::Error as StdError;
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::backend::OutputChannel;
use crate::envelope::{ExecutedInvocation, SchemaVersion};
use crate::verb::VerbId;

/// Convenience alias.
pub type Result<T, E = DriverError> = std::result::Result<T, E>;

/// Everything that can go wrong driving the CLI.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum DriverError {
    /// Binary lookup failed.
    #[error("backhopper binary not found (searched {searched:?})")]
    BinaryNotFound {
        /// Paths the discoverer searched in order.
        searched: Vec<PathBuf>,
        #[source]
        /// The last `io::Error` returned by an attempt to stat the
        /// binary.
        source: io::Error,
    },

    /// Spawn failed (binary present but unusable).
    #[error("failed to spawn backhopper at {binary_path:?}")]
    Spawn {
        /// Path the backend tried to spawn.
        binary_path: PathBuf,
        #[source]
        /// Underlying `io::Error`.
        source: io::Error,
    },

    /// Per-invocation timeout expired.
    #[error("backhopper invocation timed out after {after:?}: {verb}")]
    Timeout {
        /// Which verb timed out.
        verb: VerbId<'static>,
        /// How long the backend waited.
        after: Duration,
        /// The argv the timed-out process ran, so the caller can record
        /// the exact command.
        argv: Vec<OsString>,
    },

    /// Caller-supplied cancellation token was tripped.
    #[error("backhopper invocation cancelled: {verb}")]
    Cancelled {
        /// Which verb was cancelled.
        verb: VerbId<'static>,
        /// The argv the cancelled process ran.
        argv: Vec<OsString>,
    },

    /// The CLI exited with a code that is not success-shaped.
    ///
    /// `#[non_exhaustive]`: a later field never breaks a `..` match.
    #[error("backhopper exited with status {exit_code} on {verb}: {stderr}")]
    #[non_exhaustive]
    ToolError {
        /// Verb that produced the failure.
        verb: VerbId<'static>,
        /// Process exit code.
        exit_code: i32,
        /// Captured stderr (possibly truncated when the output
        /// policy capped it).
        stderr: String,
        /// The failed run's snapshot: the same exact-rerun command a
        /// successful call returns.
        executed: ExecutedInvocation,
    },

    /// A repo-operating verb was dispatched without a resolvable
    /// repository, caught before spawn so it does not silently resolve
    /// against the current directory.
    ///
    /// `#[non_exhaustive]`: a later field never breaks a `..` match.
    #[error("verb {verb} requires a repository: set --repo-dir-path on the driver")]
    #[non_exhaustive]
    MissingRepoDir {
        /// Verb that needs a repository.
        verb: VerbId<'static>,
    },

    /// The CLI reported a schema version this driver does not support.
    #[error(
        "schema_version {got} not supported by this driver (supports {supported_min}..={supported_max})"
    )]
    SchemaVersionMismatch {
        /// Schema version the CLI emitted.
        got: SchemaVersion,
        /// Lowest supported version.
        supported_min: SchemaVersion,
        /// Highest supported version.
        supported_max: SchemaVersion,
    },

    /// The CLI's stdout did not parse into the expected payload.
    #[error("could not parse backhopper output as {expected} for {verb}: {source}")]
    UnparseableOutput {
        /// Verb that produced the output.
        verb: VerbId<'static>,
        /// Human-readable description of the expected payload type.
        expected: &'static str,
        /// Raw stdout bytes (possibly truncated).
        stdout: Vec<u8>,
        /// Raw stderr bytes (possibly truncated).
        stderr: Vec<u8>,
        #[source]
        /// The serde error.
        source: serde_json::Error,
    },

    /// A capture thread panicked; the channel's output is lost and
    /// must not read as empty.
    #[error("backhopper output reader thread panicked on {channel:?}: {verb}")]
    ReaderPanicked {
        /// Verb whose output was being captured.
        verb: VerbId<'static>,
        /// Which channel's reader died.
        channel: OutputChannel,
    },

    /// Captured output exceeded the policy cap.
    #[error("backhopper produced more than {limit} bytes on {channel:?}: {verb}")]
    OutputTooLarge {
        /// Verb that produced the output.
        verb: VerbId<'static>,
        /// Which channel exceeded the cap.
        channel: OutputChannel,
        /// The cap, in bytes.
        limit: usize,
    },

    /// Backend-level transport error (custom backends only).
    #[error("backend transport error: {source}")]
    Transport {
        #[source]
        /// The originating error.
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

impl DriverError {
    /// Coarse classification, returned as a `Copy` enum because
    /// `DriverError` itself cannot be `PartialEq` (it carries
    /// `io::Error` and `serde_json::Error`). Tests that want to
    /// assert "the error was of variant X" should match on
    /// `kind()`.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::BinaryNotFound { .. } => ErrorKind::BinaryNotFound,
            Self::Spawn { .. } => ErrorKind::Spawn,
            Self::Timeout { .. } => ErrorKind::Timeout,
            Self::Cancelled { .. } => ErrorKind::Cancelled,
            Self::ToolError { .. } => ErrorKind::ToolError,
            Self::MissingRepoDir { .. } => ErrorKind::MissingRepoDir,
            Self::SchemaVersionMismatch { .. } => ErrorKind::SchemaVersionMismatch,
            Self::UnparseableOutput { .. } => ErrorKind::UnparseableOutput,
            Self::ReaderPanicked { .. } => ErrorKind::ReaderPanicked,
            Self::OutputTooLarge { .. } => ErrorKind::OutputTooLarge,
            Self::Transport { .. } => ErrorKind::Transport,
        }
    }
}

/// Coarse `DriverError` discriminant; see [`DriverError::kind`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The CLI binary was not found on any searched path.
    BinaryNotFound,
    /// The child could not be spawned, or its stdin could not be written.
    Spawn,
    /// The invocation outlived its timeout and was killed.
    Timeout,
    /// The caller's cancellation token fired before the child finished.
    Cancelled,
    /// The CLI ran but exited with a non-zero status.
    ToolError,
    /// A repo-requiring verb was dispatched without a `--repo-dir-path`.
    MissingRepoDir,
    /// The envelope schema version is outside the supported range.
    SchemaVersionMismatch,
    /// The stdout could not be parsed into the expected envelope.
    UnparseableOutput,
    /// A stdout or stderr reader thread panicked.
    ReaderPanicked,
    /// The child wrote more than the capture limit on a channel.
    OutputTooLarge,
    /// An OS-level transport error moving bytes to or from the child.
    Transport,
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BinaryNotFound => "binary_not_found",
            Self::Spawn => "spawn",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::ToolError => "tool_error",
            Self::MissingRepoDir => "missing_repo_dir",
            Self::SchemaVersionMismatch => "schema_version_mismatch",
            Self::UnparseableOutput => "unparseable_output",
            Self::ReaderPanicked => "reader_panicked",
            Self::OutputTooLarge => "output_too_large",
            Self::Transport => "transport",
        })
    }
}
