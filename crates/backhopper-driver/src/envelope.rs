// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! JSON envelope and post-execution diagnostic snapshot.
//!
//! Every `backhopper` invocation returns
//! `{schema_version, command, data, exit_code[, warnings]}` on
//! stdout. The driver parses that into [`Envelope<T>`] over a
//! caller-supplied payload type and, alongside it, captures an
//! [`ExecutedInvocation`] describing the run (argv, duration, byte
//! counts, exit code). The envelope is opaque so the driver can add
//! envelope-level fields without a major version bump.

use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::backend::RawOutcome;
use crate::exit::ExitClass;
use crate::stdin::StdinPayload;
use crate::verb::{Verb, VerbId};

pub use backhopper_core::model::wire_envelope::EnvelopeWarning;

/// Versioned schema identifier on every CLI envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Construct from a raw integer.
    #[must_use]
    pub const fn new(version: u32) -> Self {
        Self(version)
    }

    /// The raw integer.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Display for SchemaVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

/// A parsed JSON envelope from a `backhopper` invocation.
///
/// Opaque by design: callers obtain values via accessor methods so
/// the driver can grow envelope-level fields (for example, a future
/// `warnings` array or a server-timing block) without a major
/// version bump.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[must_use = "an Envelope carries the verb's payload"]
pub struct Envelope<T> {
    schema_version: SchemaVersion,
    command: Option<Verb>,
    command_raw: String,
    data: T,
    exit_code: i32,
    warnings: Vec<EnvelopeWarning>,
    executed: ExecutedInvocation,
}

impl<T> Envelope<T> {
    /// Build an envelope. Constructor exists so the parser (in
    /// `crate::driver`) and `MockBackend` can synthesise envelopes
    /// without exposing the field shape.
    pub(crate) fn new(
        schema_version: SchemaVersion,
        command: Option<Verb>,
        command_raw: String,
        data: T,
        exit_code: i32,
        warnings: Vec<EnvelopeWarning>,
        executed: ExecutedInvocation,
    ) -> Self {
        Self {
            schema_version,
            command,
            command_raw,
            data,
            exit_code,
            warnings,
            executed,
        }
    }

    /// Schema version the CLI emitted.
    pub fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// The verb the CLI's `command` string parsed to, `None` when it
    /// names a verb this driver does not know. See [`Self::command_raw`]
    /// for the string as emitted.
    pub fn command(&self) -> Option<Verb> {
        self.command
    }

    /// The raw `command` string the CLI emitted, whether or not it
    /// parsed to a known [`Verb`].
    pub fn command_raw(&self) -> &str {
        &self.command_raw
    }

    /// Exit code carried inside the JSON envelope. The process
    /// itself exits with the same code, so callers can ignore this
    /// when they have direct access to the child status.
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    /// Borrow the typed payload.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Consume the envelope and return the typed payload.
    #[must_use]
    pub fn into_data(self) -> T {
        self.data
    }

    /// Free-form CLI warnings carried alongside the payload.
    pub fn warnings(&self) -> &[EnvelopeWarning] {
        &self.warnings
    }

    /// Post-execution snapshot (argv, duration, byte counts).
    pub fn executed(&self) -> &ExecutedInvocation {
        &self.executed
    }

    /// Convenience: the argv the backend actually ran.
    pub fn argv(&self) -> &[OsString] {
        &self.executed.argv
    }

    /// Convenience: wall-clock duration of the invocation.
    pub fn duration(&self) -> Duration {
        self.executed.duration
    }

    /// Apply `f` to the payload while preserving envelope metadata.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Envelope<U> {
        Envelope {
            schema_version: self.schema_version,
            command: self.command,
            command_raw: self.command_raw,
            data: f(self.data),
            exit_code: self.exit_code,
            warnings: self.warnings,
            executed: self.executed,
        }
    }

    /// Split the envelope into its parts. The verb is
    /// `None` when the `command` string named an unknown verb; the raw
    /// string stays reachable through [`Self::command_raw`] before this.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        SchemaVersion,
        Option<Verb>,
        T,
        i32,
        Vec<EnvelopeWarning>,
        ExecutedInvocation,
    ) {
        (
            self.schema_version,
            self.command,
            self.data,
            self.exit_code,
            self.warnings,
            self.executed,
        )
    }
}

/// Post-execution snapshot of an invocation.
///
/// Returned by every type-state builder's `run_with_diagnostics()`
/// and accessible via [`Envelope::executed`] when callers use the
/// escape-hatch `invoke` path. Carries everything a downstream tool
/// needs to write a diagnostic block alongside the verdict.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ExecutedInvocation {
    /// Verb the driver dispatched.
    pub verb: VerbId<'static>,
    /// Full argv the backend executed, including the program path
    /// at index 0 (when known) and any driver-injected flags
    /// (`--formatter json`, `--non-interactive`).
    pub argv: Vec<OsString>,
    /// Byte count of the stdin payload, or `None` if no stdin was
    /// piped. Deliberately a count: patches and commit lists are
    /// often large and copying them into the diagnostic record is
    /// rarely worth the cost.
    pub stdin_bytes: Option<usize>,
    /// Process exit code.
    pub exit_code: i32,
    /// `ExitClass::from_code(exit_code)`. Cached for callers that
    /// would otherwise re-classify the integer.
    pub exit_class: ExitClass,
    /// Wall-clock duration of the invocation.
    pub duration: Duration,
    /// Bytes captured from stdout.
    pub stdout_bytes: usize,
    /// Bytes captured from stderr.
    pub stderr_bytes: usize,
    /// When the invocation started.
    pub started_at: SystemTime,
}

impl ExecutedInvocation {
    /// Synthesise a snapshot from a [`RawOutcome`] plus the originating
    /// verb and stdin descriptor. A convenience constructor for callers
    /// driving a backend directly; the in-tree driver builds this inline
    /// to move `argv` without a clone.
    #[must_use]
    pub fn from_outcome(
        verb: VerbId<'static>,
        stdin: &StdinPayload<'_>,
        outcome: &RawOutcome,
        started_at: SystemTime,
    ) -> Self {
        let stdin_bytes = match stdin {
            StdinPayload::None => None,
            StdinPayload::Bytes(b) => Some(b.len()),
            StdinPayload::Reader(_) => None,
        };
        Self {
            verb,
            argv: outcome.argv.clone(),
            stdin_bytes,
            exit_code: outcome.exit_code,
            exit_class: ExitClass::from_code(outcome.exit_code),
            duration: outcome.duration,
            stdout_bytes: outcome.stdout.len(),
            stderr_bytes: outcome.stderr.len(),
            started_at,
        }
    }
}
