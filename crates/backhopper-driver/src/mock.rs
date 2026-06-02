// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Test-only mock backend.
//!
//! Gated behind the `mock` Cargo feature so it never ships in
//! release binaries unless callers opt in. Configure via a
//! [`MockBackendBuilder`]: each handler matches an invocation and
//! produces a [`MockResponse`]. The backend records every
//! invocation it sees for after-the-fact assertions.

use std::ffi::OsString;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::backend::{Backend, Invocation, RawOutcome};
use crate::envelope::{EnvelopeWarning, SchemaVersion};
use crate::error::DriverError;
use crate::stdin::StdinPayload;
use crate::verb::{Verb, VerbId};

/// Where unmatched invocations go.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fallthrough {
    /// Return [`DriverError::Transport`] describing the unmatched
    /// invocation. The default and the most useful in tests.
    #[default]
    Error,
    /// Return an empty success outcome.
    Empty,
    /// Panic. Useful when tests should assert the driver only emits
    /// the invocations the test explicitly wired up.
    Panic,
}

/// Predicate describing the invocations a handler claims.
pub enum MockMatcher {
    /// Any invocation of `verb`.
    Verb(Verb),
    /// `verb` invocations that also satisfy `predicate`.
    VerbWhere {
        /// Verb that gates the predicate.
        verb: Verb,
        /// Predicate over the invocation.
        predicate: Arc<dyn Fn(&Invocation<'_>) -> bool + Send + Sync>,
    },
    /// Any invocation that `predicate` accepts.
    Custom(Arc<dyn Fn(&Invocation<'_>) -> bool + Send + Sync>),
}

impl fmt::Debug for MockMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verb(v) => write!(f, "Verb({v})"),
            Self::VerbWhere { verb, .. } => write!(f, "VerbWhere {{ verb: {verb}, .. }}"),
            Self::Custom(_) => f.write_str("Custom(_)"),
        }
    }
}

impl MockMatcher {
    fn matches(&self, invocation: &Invocation<'_>) -> bool {
        let known = invocation.verb.as_known();
        match self {
            Self::Verb(v) => known == Some(*v),
            Self::VerbWhere { verb, predicate } => known == Some(*verb) && predicate(invocation),
            Self::Custom(predicate) => predicate(invocation),
        }
    }
}

/// What a matched handler returns.
#[non_exhaustive]
pub enum MockResponse {
    /// JSON envelope, exit code, schema version, optional warnings.
    Json {
        /// Payload bytes will be `data` inside the envelope.
        payload: Value,
        /// Process exit code.
        exit_code: i32,
        /// Envelope schema version.
        schema_version: SchemaVersion,
        /// Optional envelope warnings.
        warnings: Vec<EnvelopeWarning>,
    },
    /// A pre-built [`RawOutcome`] returned verbatim.
    Raw(RawOutcome),
    /// Return a specific [`DriverError`].
    Error(DriverError),
    /// Sleep for `duration`, then yield `then`. Useful for
    /// exercising timeout and cancellation handling.
    DelayedThen {
        /// How long to sleep before responding.
        duration: Duration,
        /// Wrapped response (single Box for size).
        then: Box<MockResponse>,
    },
}

impl fmt::Debug for MockResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json {
                exit_code,
                schema_version,
                ..
            } => write!(
                f,
                "Json {{ exit_code: {exit_code}, schema_version: {schema_version}, .. }}"
            ),
            Self::Raw(o) => write!(
                f,
                "Raw({} bytes stdout, exit {})",
                o.stdout.len(),
                o.exit_code
            ),
            Self::Error(e) => write!(f, "Error({})", e.kind()),
            Self::DelayedThen { duration, .. } => write!(f, "DelayedThen({duration:?}, ..)"),
        }
    }
}

impl MockResponse {
    /// Build a JSON success response with the driver's default
    /// schema version and exit code `0`.
    #[must_use]
    pub fn json_ok(payload: Value) -> Self {
        Self::Json {
            payload,
            exit_code: 0,
            schema_version: SchemaVersion::new(1),
            warnings: Vec::new(),
        }
    }

    /// Build a JSON response with an explicit exit code.
    #[must_use]
    pub fn json_with_code(payload: Value, code: i32) -> Self {
        Self::Json {
            payload,
            exit_code: code,
            schema_version: SchemaVersion::new(1),
            warnings: Vec::new(),
        }
    }

    /// Build a raw success outcome.
    #[must_use]
    pub fn raw_ok(stdout: impl Into<Vec<u8>>) -> Self {
        Self::Raw(RawOutcome::ok(stdout))
    }

    /// Build a tool-error response. The caller-supplied stderr ends
    /// up inside [`DriverError::ToolError::stderr`].
    #[must_use]
    pub fn tool_error(stderr: impl Into<String>, code: i32) -> Self {
        Self::Raw(RawOutcome {
            exit_code: code,
            stdout: Vec::new(),
            stderr: stderr.into().into_bytes(),
            duration: Duration::ZERO,
            argv: Vec::new(),
        })
    }
}

/// Recorded record of one invocation the backend serviced.
#[derive(Debug, Clone)]
pub struct RecordedInvocation {
    /// Matched verb.
    pub verb: VerbId<'static>,
    /// Argument values (verb path excluded; matches `Invocation::args`).
    pub args: Vec<OsString>,
    /// Captured stdin bytes (a copy when the source was `Bytes`,
    /// `None` for `None` or `Reader`).
    pub stdin: Option<Vec<u8>>,
    /// When the backend received the invocation.
    pub when: SystemTime,
}

/// Mockable backend for tests.
pub struct MockBackend {
    handlers: Vec<Handler>,
    fallthrough: Fallthrough,
    log: Mutex<Vec<RecordedInvocation>>,
}

impl fmt::Debug for MockBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockBackend")
            .field("handlers", &self.handlers.len())
            .field("fallthrough", &self.fallthrough)
            .finish_non_exhaustive()
    }
}

struct Handler {
    matcher: MockMatcher,
    responder: Arc<dyn Fn(&Invocation<'_>) -> MockResponse + Send + Sync>,
}

impl MockBackend {
    /// Start a builder.
    pub fn builder() -> MockBackendBuilder {
        MockBackendBuilder::default()
    }

    /// Snapshot the recorded invocation log.
    #[must_use]
    pub fn invocations(&self) -> Vec<RecordedInvocation> {
        self.log.lock().expect("log poisoned").clone()
    }

    /// Drop every recorded invocation; the handlers are unchanged.
    pub fn clear_log(&self) {
        self.log.lock().expect("log poisoned").clear();
    }

    fn record(&self, invocation: &Invocation<'_>) {
        let stdin = match &invocation.stdin {
            StdinPayload::Bytes(b) => Some(b.to_vec()),
            _ => None,
        };
        let args: Vec<OsString> = invocation.args.iter().map(|a| a.to_os_string()).collect();
        self.log
            .lock()
            .expect("log poisoned")
            .push(RecordedInvocation {
                verb: invocation.verb.clone().into_owned(),
                args,
                stdin,
                when: SystemTime::now(),
            });
    }
}

impl Backend for MockBackend {
    fn invoke(&self, invocation: Invocation<'_>) -> Result<RawOutcome, DriverError> {
        self.record(&invocation);
        let response = match self
            .handlers
            .iter()
            .find(|h| h.matcher.matches(&invocation))
        {
            Some(handler) => (handler.responder)(&invocation),
            None => match self.fallthrough {
                Fallthrough::Error => {
                    return Err(DriverError::Transport {
                        source: format!(
                            "MockBackend: unmatched invocation: {}",
                            invocation.verb.display_name()
                        )
                        .into(),
                    });
                }
                Fallthrough::Empty => return Ok(RawOutcome::ok(b"".to_vec())),
                Fallthrough::Panic => {
                    panic!(
                        "MockBackend: unmatched invocation: {}",
                        invocation.verb.display_name()
                    );
                }
            },
        };
        materialise(response, &invocation)
    }
}

fn materialise(
    response: MockResponse,
    invocation: &Invocation<'_>,
) -> Result<RawOutcome, DriverError> {
    match response {
        MockResponse::Json {
            payload,
            exit_code,
            schema_version,
            warnings,
        } => {
            let command_path = invocation.verb.display_name();
            let warnings_json: Vec<Value> = warnings
                .into_iter()
                .map(|w| json!({ "code": w.code, "message": w.message }))
                .collect();
            let envelope = json!({
                "schema_version": schema_version.get(),
                "command": command_path,
                "data": payload,
                "exit_code": exit_code,
                "warnings": warnings_json,
            });
            let mut argv = Vec::new();
            argv.push(OsString::from("backhopper"));
            for seg in invocation.verb.cli_path() {
                argv.push(OsString::from(seg));
            }
            for a in invocation.args.iter() {
                argv.push(a.to_os_string());
            }
            Ok(RawOutcome {
                exit_code,
                stdout: serde_json::to_vec(&envelope).expect("envelope serialises"),
                stderr: Vec::new(),
                duration: Duration::ZERO,
                argv,
            })
        }
        MockResponse::Raw(mut outcome) => {
            if outcome.argv.is_empty() {
                outcome.argv.push(OsString::from("backhopper"));
                for seg in invocation.verb.cli_path() {
                    outcome.argv.push(OsString::from(seg));
                }
                for a in invocation.args.iter() {
                    outcome.argv.push(a.to_os_string());
                }
            }
            Ok(outcome)
        }
        MockResponse::Error(err) => Err(err),
        MockResponse::DelayedThen { duration, then } => {
            thread::sleep(duration);
            materialise(*then, invocation)
        }
    }
}

/// Chained constructor for [`MockBackend`].
#[derive(Default)]
#[must_use = "build() consumes the builder; assign the result"]
pub struct MockBackendBuilder {
    handlers: Vec<Handler>,
    fallthrough: Fallthrough,
}

impl fmt::Debug for MockBackendBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockBackendBuilder")
            .field("handlers", &self.handlers.len())
            .field("fallthrough", &self.fallthrough)
            .finish()
    }
}

impl MockBackendBuilder {
    /// Add a handler that returns the same response every time.
    pub fn respond_to(mut self, matcher: MockMatcher, response: MockResponse) -> Self {
        let response = Arc::new(Mutex::new(Some(response)));
        let response_cell = Arc::clone(&response);
        let responder = Arc::new(move |_inv: &Invocation<'_>| {
            let mut slot = response_cell.lock().expect("response slot poisoned");
            if let Some(r) = slot.take() {
                let copy = clone_response(&r);
                *slot = Some(r);
                copy
            } else {
                MockResponse::Raw(RawOutcome::ok(b"".to_vec()))
            }
        });
        self.handlers.push(Handler { matcher, responder });
        self
    }

    /// Add a handler whose response is computed from the invocation.
    pub fn respond_with(
        mut self,
        matcher: MockMatcher,
        f: impl Fn(&Invocation<'_>) -> MockResponse + Send + Sync + 'static,
    ) -> Self {
        self.handlers.push(Handler {
            matcher,
            responder: Arc::new(f),
        });
        self
    }

    /// Set the fallthrough behaviour.
    pub fn fallthrough(mut self, behaviour: Fallthrough) -> Self {
        self.fallthrough = behaviour;
        self
    }

    /// Finalise.
    #[must_use]
    pub fn build(self) -> MockBackend {
        MockBackend {
            handlers: self.handlers,
            fallthrough: self.fallthrough,
            log: Mutex::new(Vec::new()),
        }
    }
}

fn clone_response(response: &MockResponse) -> MockResponse {
    match response {
        MockResponse::Json {
            payload,
            exit_code,
            schema_version,
            warnings,
        } => MockResponse::Json {
            payload: payload.clone(),
            exit_code: *exit_code,
            schema_version: *schema_version,
            warnings: warnings.clone(),
        },
        MockResponse::Raw(o) => MockResponse::Raw(o.clone()),
        MockResponse::Error(e) => MockResponse::Error(transport_clone(e)),
        MockResponse::DelayedThen { duration, then } => MockResponse::DelayedThen {
            duration: *duration,
            then: Box::new(clone_response(then)),
        },
    }
}

fn transport_clone(err: &DriverError) -> DriverError {
    DriverError::Transport {
        source: format!("{err}").into(),
    }
}
