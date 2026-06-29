// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Backend trait, invocation descriptor, and raw outcome.
//!
//! A [`Backend`] runs one invocation. Implementations live in
//! [`crate::subprocess`] (the real subprocess) and [`crate::mock`]
//! (the test-only mock, gated behind the `mock` feature). Custom
//! backends are welcome: the trait is small and the [`Invocation`]
//! struct is `#[non_exhaustive]`, so additions do not break
//! implementors.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;
use std::time::Duration;

use crate::cancellation::CancellationToken;
use crate::error::DriverError;
use crate::options::EnvOverlay;
use crate::stdin::StdinPayload;
use crate::verb::VerbId;

/// Run a single `backhopper` invocation and return its raw outcome.
///
/// The trait is `Send + Sync` so `Backhopper<B>` is shareable across
/// threads when `B` is too, and object-safe so `Box<dyn Backend>`
/// works for callers that need to swap backends at runtime.
pub trait Backend: Send + Sync {
    /// Run the invocation. Implementations must populate
    /// [`RawOutcome::argv`] with the argv they actually executed so
    /// the driver can build diagnostic snapshots.
    fn invoke(&self, invocation: Invocation<'_>) -> Result<RawOutcome, DriverError>;
}

/// What the backend should run, plus per-invocation policies the
/// driver layered on top of [`GlobalOptions`].
///
/// [`GlobalOptions`]: crate::options::GlobalOptions
#[non_exhaustive]
pub struct Invocation<'a> {
    /// Which verb (typed or custom).
    pub verb: VerbId<'a>,
    /// Argument list passed after the verb path (e.g. flag values).
    pub args: Cow<'a, [Cow<'a, OsStr>]>,
    /// Standard-input source.
    pub stdin: StdinPayload<'a>,
    /// How much output to buffer.
    pub stdout_policy: OutputPolicy,
    /// How much stderr to buffer.
    pub stderr_policy: OutputPolicy,
    /// Per-invocation timeout.
    pub timeout: Option<Duration>,
    /// Cooperative cancellation.
    pub cancellation: Option<&'a CancellationToken>,
    /// Working directory.
    pub working_directory: Option<&'a Path>,
    /// Environment overlay.
    pub env: &'a EnvOverlay,
}

impl fmt::Debug for Invocation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invocation")
            .field("verb", &self.verb.display_name())
            .field("args", &self.args.len())
            .field("stdin", &self.stdin)
            .field("timeout", &self.timeout)
            .field("working_directory", &self.working_directory)
            .finish_non_exhaustive()
    }
}

impl<'a> Invocation<'a> {
    /// Construct an invocation with sensible defaults for the
    /// per-invocation policy fields. Callers (typically the driver)
    /// fill in only what they care about.
    #[must_use]
    pub fn new(verb: VerbId<'a>, env: &'a EnvOverlay) -> Self {
        Self {
            verb,
            args: Cow::Owned(Vec::new()),
            stdin: StdinPayload::None,
            stdout_policy: OutputPolicy::default(),
            stderr_policy: OutputPolicy::default(),
            timeout: None,
            cancellation: None,
            working_directory: None,
            env,
        }
    }

    /// Replace the argument list.
    #[must_use]
    pub fn with_args(mut self, args: impl Into<Cow<'a, [Cow<'a, OsStr>]>>) -> Self {
        self.args = args.into();
        self
    }

    /// Replace the stdin payload.
    #[must_use]
    pub fn with_stdin(mut self, stdin: StdinPayload<'a>) -> Self {
        self.stdin = stdin;
        self
    }

    /// Replace the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Attach a cancellation token.
    #[must_use]
    pub fn with_cancellation(mut self, token: Option<&'a CancellationToken>) -> Self {
        self.cancellation = token;
        self
    }

    /// Run the child in the named working directory.
    #[must_use]
    pub fn with_working_directory(mut self, dir: Option<&'a Path>) -> Self {
        self.working_directory = dir;
        self
    }
}

/// Build the argv a backend reports in `RawOutcome::argv`: the program,
/// the verb's CLI path segments, then the argument list. One shape so
/// the subprocess backend and the mock cannot drift.
pub(crate) fn assemble_argv(
    program: &OsStr,
    verb: &VerbId<'_>,
    args: &[Cow<'_, OsStr>],
) -> Vec<OsString> {
    let mut argv = Vec::with_capacity(2 + args.len());
    argv.push(program.to_owned());
    for seg in verb.cli_path() {
        argv.push(OsString::from(seg));
    }
    for arg in args {
        argv.push(arg.to_os_string());
    }
    argv
}

/// How much output to buffer from one of the child's streams.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPolicy {
    /// Buffer everything. Convenient default; risky for verbs that
    /// can produce arbitrarily large output.
    Buffer,
    /// Buffer up to the named cap; produce
    /// [`DriverError::OutputTooLarge`] when the cap is exceeded.
    BufferUpTo(usize),
    /// Discard. Useful when the only contract is the exit code.
    Discard,
}

impl Default for OutputPolicy {
    fn default() -> Self {
        Self::BufferUpTo(256 * 1024 * 1024)
    }
}

/// Which output channel exceeded its policy cap.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputChannel {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Result of a backend invocation.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[must_use = "RawOutcome carries the verb's payload"]
pub struct RawOutcome {
    /// Process exit code.
    pub exit_code: i32,
    /// Captured stdout (subject to the stdout `OutputPolicy`).
    pub stdout: Vec<u8>,
    /// Captured stderr (subject to the stderr `OutputPolicy`).
    pub stderr: Vec<u8>,
    /// Wall-clock duration of the invocation.
    pub duration: Duration,
    /// Full argv the backend actually executed (program path at
    /// index 0 when known, then the verb path, then arguments).
    pub argv: Vec<OsString>,
}

impl RawOutcome {
    /// Minimal helper for ad-hoc construction (mainly used by mock
    /// fixtures and tests).
    pub fn ok(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            argv: Vec::new(),
        }
    }

    /// Override the exit code.
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = code;
        self
    }

    /// Override the stderr buffer.
    pub fn with_stderr(mut self, stderr: impl Into<Vec<u8>>) -> Self {
        self.stderr = stderr.into();
        self
    }

    /// Override the duration.
    pub fn with_duration(mut self, d: Duration) -> Self {
        self.duration = d;
        self
    }

    /// Override the argv.
    pub fn with_argv(mut self, argv: impl IntoIterator<Item = OsString>) -> Self {
        self.argv = argv.into_iter().collect();
        self
    }

    /// Convenience read-only accessor.
    pub fn argv(&self) -> &[OsString] {
        &self.argv
    }
}
