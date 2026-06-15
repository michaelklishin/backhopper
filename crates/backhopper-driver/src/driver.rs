// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Top-level driver type.
//!
//! `Backhopper<B>` owns a [`Backend`] plus shared [`GlobalOptions`].
//! Verb groups (`config`, `snapshots`, `check`) are reached via
//! short borrow wrappers that build a single invocation each.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::mem;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::backend::{Backend, Invocation, OutputPolicy, RawOutcome};
use crate::builder::check::{Check, SeriesEvaluation};
use crate::builder::siblings::Siblings;
use crate::builder::snapshots::Snapshots;
use crate::builder::suites::Suites;
use crate::envelope::{Envelope, EnvelopeWarning, ExecutedInvocation, SchemaVersion};
use crate::error::DriverError;
use crate::exit::ExitClass;
use crate::options::GlobalOptions;
use crate::stdin::StdinPayload;
use crate::subprocess::{SubprocessBackend, discover_binary};
use crate::verb::{Verb, VerbId};

/// Entry point for driving a `backhopper` binary from Rust.
#[must_use = "construct a Backhopper and call its verbs to do useful work"]
pub struct Backhopper<B: Backend = SubprocessBackend> {
    backend: B,
    options: GlobalOptions,
    cached_version: OnceLock<VersionInfo>,
}

impl<B: Backend + fmt::Debug> fmt::Debug for Backhopper<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Backhopper")
            .field("backend", &self.backend)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Backhopper<SubprocessBackend> {
    /// Locate the `backhopper` binary by consulting
    /// `$BACKHOPPER_BINARY_PATH`, then `$PATH`.
    pub fn auto_discover() -> Result<Self, DriverError> {
        let path = discover_binary()?;
        Ok(Self::with_binary_path(path))
    }

    /// Use the binary at `path` without searching.
    pub fn with_binary_path(path: impl Into<PathBuf>) -> Self {
        Self {
            backend: SubprocessBackend::new(path),
            options: GlobalOptions::non_interactive_default(),
            cached_version: OnceLock::new(),
        }
    }
}

impl<B: Backend> Backhopper<B> {
    /// Construct over a caller-supplied backend (typically a mock).
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend,
            options: GlobalOptions::non_interactive_default(),
            cached_version: OnceLock::new(),
        }
    }

    /// Adopt a new set of options at construction time.
    pub fn with_options(mut self, options: GlobalOptions) -> Self {
        self.options = options;
        self
    }

    /// Borrow the global options.
    #[must_use]
    pub fn options(&self) -> &GlobalOptions {
        &self.options
    }

    /// Mutate the global options in place.
    pub fn options_mut(&mut self) -> &mut GlobalOptions {
        &mut self.options
    }

    /// Borrow the backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Probe the binary's version. Cached on the first call so the
    /// driver does not re-spawn for repeated `version()` checks.
    pub fn version(&self) -> Result<&VersionInfo, DriverError> {
        if let Some(cached) = self.cached_version.get() {
            return Ok(cached);
        }
        let env =
            self.invoke::<Value, &OsStr>(VerbId::Known(Verb::Version), &[], StdinPayload::None)?;
        let info = parse_version_info(env.schema_version(), env.data());
        let _ = self.cached_version.set(info);
        Ok(self.cached_version.get().expect("just set"))
    }

    /// Probe the binary's compatibility, returning a report rather than
    /// failing on a mismatch.
    ///
    /// Reads the `version` envelope leniently: the strict path rejects
    /// an out-of-range `schema_version`, which is the mismatch this
    /// check exists to report cleanly. `required` names the verbs the
    /// caller depends on.
    pub fn compatibility(&self, required: &[&str]) -> Result<Compatibility, DriverError> {
        // `invoke_raw` skips the envelope path, so it does not inject
        // `--formatter json`; pass it so the version payload is JSON.
        let outcome = self.invoke_raw(
            VerbId::Known(Verb::Version),
            [OsStr::new("--formatter"), OsStr::new("json")],
            StdinPayload::None,
        )?;
        let value = serde_json::from_slice::<Value>(&outcome.stdout).ok();
        let binary_schema = value
            .as_ref()
            .and_then(|v| v.get("schema_version"))
            .and_then(Value::as_u64)
            .map(|n| SchemaVersion::new(n as u32));
        let data = value
            .and_then(|v| v.get("data").cloned())
            .unwrap_or(Value::Null);
        let info = parse_version_info(binary_schema.unwrap_or(SchemaVersion::new(0)), &data);
        let missing_verbs = info.missing_verbs(required);
        Ok(Compatibility {
            binary_version: info.version,
            binary_schema,
            supported: crate::SUPPORTED_SCHEMA,
            missing_verbs,
        })
    }

    /// Reach into the `check` verb group.
    pub fn check(&self) -> Check<'_, B> {
        Check { driver: self }
    }

    /// Reach into the `siblings` verb group.
    pub fn siblings(&self) -> Siblings<'_, B> {
        Siblings { driver: self }
    }

    /// Reach into the `suites` verb group.
    pub fn suites(&self) -> Suites<'_, B> {
        Suites { driver: self }
    }

    /// Reach into the `snapshots` verb group.
    pub fn snapshots(&self) -> Snapshots<'_, B> {
        Snapshots { driver: self }
    }

    /// Run an arbitrary verb and return the raw outcome. Use this
    /// for verbs the driver does not yet wrap.
    pub fn invoke_raw<'a, A: AsRef<OsStr>>(
        &'a self,
        verb: VerbId<'a>,
        args: impl IntoIterator<Item = A>,
        stdin: StdinPayload<'a>,
    ) -> Result<RawOutcome, DriverError> {
        let mut owned_args: Vec<Cow<'a, OsStr>> = Vec::new();
        push_global_flags(&self.options, &mut owned_args);
        for arg in args {
            owned_args.push(Cow::Owned(arg.as_ref().to_os_string()));
        }
        let invocation = Invocation {
            verb,
            args: Cow::Owned(owned_args),
            stdin,
            stdout_policy: OutputPolicy::default(),
            stderr_policy: OutputPolicy::default(),
            timeout: self.options.timeout,
            cancellation: self.options.cancellation.as_ref(),
            working_directory: self.options.working_directory.as_deref(),
            env: &self.options.env,
        };
        self.backend.invoke(invocation)
    }

    /// Run an arbitrary verb and parse the JSON envelope into `T`.
    pub fn invoke<'a, T, A>(
        &'a self,
        verb: VerbId<'a>,
        args: &[A],
        stdin: StdinPayload<'a>,
    ) -> Result<Envelope<T>, DriverError>
    where
        T: DeserializeOwned,
        A: AsRef<OsStr>,
    {
        let mut assembled: Vec<Cow<'a, OsStr>> = Vec::with_capacity(args.len() + 10);
        push_global_flags(&self.options, &mut assembled);
        assembled.push(Cow::Borrowed(OsStr::new("--formatter")));
        assembled.push(Cow::Borrowed(OsStr::new("json")));
        for arg in args {
            assembled.push(Cow::Owned(arg.as_ref().to_os_string()));
        }
        self.dispatch_envelope(verb, assembled, stdin)
    }

    /// Shared typed-dispatch path: one `(T, ExecutedInvocation)` result
    /// for every builder, with the args vector moved in without a clone.
    pub(crate) fn dispatch_typed<'a, T: DeserializeOwned>(
        &'a self,
        verb: Verb,
        args: Vec<OsString>,
        stdin: StdinPayload<'a>,
    ) -> Result<(T, ExecutedInvocation), DriverError> {
        // Refuse a repo-operating verb with no repo dir before spawn,
        // rather than let the CLI default to the current directory.
        if verb.requires_global_repo_dir() && self.options.repo_dir_path.is_none() {
            return Err(DriverError::MissingRepoDir {
                verb: VerbId::Known(verb),
            });
        }
        let mut assembled: Vec<Cow<'a, OsStr>> = Vec::with_capacity(args.len() + 10);
        push_global_flags(&self.options, &mut assembled);
        assembled.push(Cow::Borrowed(OsStr::new("--formatter")));
        assembled.push(Cow::Borrowed(OsStr::new("json")));
        for arg in args {
            assembled.push(Cow::Owned(arg));
        }
        let env = self.dispatch_envelope::<T>(VerbId::Known(verb), assembled, stdin)?;
        let (_, _, data, _, _, executed) = env.into_parts();
        Ok((data, executed))
    }

    /// `SeriesEvaluation`-typed dispatch for the `check` builders.
    pub(crate) fn dispatch_check<'a>(
        &'a self,
        verb: Verb,
        args: Vec<OsString>,
        stdin: StdinPayload<'a>,
    ) -> Result<(SeriesEvaluation, ExecutedInvocation), DriverError> {
        self.dispatch_typed::<SeriesEvaluation>(verb, args, stdin)
    }

    fn dispatch_envelope<'a, T: DeserializeOwned>(
        &'a self,
        verb: VerbId<'a>,
        args: Vec<Cow<'a, OsStr>>,
        stdin: StdinPayload<'a>,
    ) -> Result<Envelope<T>, DriverError> {
        let owned_verb_for_record = verb.clone().into_owned();
        let started = SystemTime::now();
        let stdin_snapshot = match &stdin {
            StdinPayload::None => StdinSnapshot::None,
            StdinPayload::Bytes(b) => StdinSnapshot::Bytes(b.len()),
            StdinPayload::Reader(_) => StdinSnapshot::Reader,
        };
        let invocation = Invocation {
            verb,
            args: Cow::Owned(args),
            stdin,
            stdout_policy: OutputPolicy::default(),
            stderr_policy: OutputPolicy::default(),
            timeout: self.options.timeout,
            cancellation: self.options.cancellation.as_ref(),
            working_directory: self.options.working_directory.as_deref(),
            env: &self.options.env,
        };
        let mut outcome = self.backend.invoke(invocation)?;

        let exit_class = ExitClass::from_code(outcome.exit_code);
        // Snapshot before the success check: a failed run reports the
        // same argv a success does.
        let executed = ExecutedInvocation {
            verb: owned_verb_for_record.clone(),
            argv: mem::take(&mut outcome.argv),
            stdin_bytes: match stdin_snapshot {
                StdinSnapshot::None | StdinSnapshot::Reader => None,
                StdinSnapshot::Bytes(n) => Some(n),
            },
            exit_code: outcome.exit_code,
            exit_class,
            duration: outcome.duration,
            stdout_bytes: outcome.stdout.len(),
            stderr_bytes: outcome.stderr.len(),
            started_at: started,
        };

        if !exit_class.is_success_shaped() {
            return Err(DriverError::ToolError {
                verb: owned_verb_for_record,
                exit_code: outcome.exit_code,
                stderr: String::from_utf8_lossy(&outcome.stderr).into_owned(),
                executed,
            });
        }

        parse_envelope::<T>(owned_verb_for_record, &outcome, executed)
    }
}

fn push_global_flags<'a>(options: &'a GlobalOptions, args: &mut Vec<Cow<'a, OsStr>>) {
    if let Some(p) = &options.config_path {
        args.push(Cow::Borrowed(OsStr::new("--config-file-path")));
        args.push(Cow::Borrowed(p.as_os_str()));
    }
    if let Some(p) = &options.snapshot_dir {
        args.push(Cow::Borrowed(OsStr::new("--snapshot-dir-path")));
        args.push(Cow::Borrowed(p.as_os_str()));
    }
    if let Some(p) = &options.repo_dir_path {
        args.push(Cow::Borrowed(OsStr::new("--repo-dir-path")));
        args.push(Cow::Borrowed(p.as_os_str()));
    }
    if options.non_interactive {
        args.push(Cow::Borrowed(OsStr::new("--non-interactive")));
    }
    if options.quiet {
        args.push(Cow::Borrowed(OsStr::new("--quiet")));
    }
    if options.verbose {
        args.push(Cow::Borrowed(OsStr::new("--verbose")));
    }
    if options.dry_run {
        args.push(Cow::Borrowed(OsStr::new("--dry-run")));
    }
}

/// Coarse version-info DTO returned by `version`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Reported version string, semver-shaped. The driver does not
    /// parse it further; consumers that need structured comparison
    /// can hand the string to whichever semver crate they already
    /// pull in.
    pub version: String,
    /// Schema version reported in the envelope.
    pub schema_version: SchemaVersion,
    /// `git describe`, when reported.
    pub git_describe: Option<String>,
    /// Capability list: every verb the binary ships, as
    /// space-separated paths (`"check batch"`, `"siblings doctor"`).
    /// Empty for binaries that predate the list.
    pub verbs: Vec<String>,
}

impl VersionInfo {
    /// True when the binary advertises `verb` (e.g.
    /// `"siblings doctor"`) in its capability list.
    #[must_use]
    pub fn has_verb(&self, verb: &str) -> bool {
        self.verbs.iter().any(|v| v == verb)
    }

    /// The subset of `required` the binary does not advertise. Empty
    /// when every required verb is present (or the binary predates the
    /// list, in which case nothing can be asserted).
    #[must_use]
    pub fn missing_verbs(&self, required: &[&str]) -> Vec<String> {
        required
            .iter()
            .filter(|r| !self.has_verb(r))
            .map(|r| (*r).to_string())
            .collect()
    }
}

/// Compatibility of an installed binary with this driver, as a report
/// the caller can display rather than a bare pass or fail.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Compatibility {
    /// Version string the binary reported.
    pub binary_version: String,
    /// Schema version the binary's `version` envelope declared. `None`
    /// when that envelope lacked `schema_version`: a binary that
    /// predates the field is below the structural floor and treated as
    /// incompatible.
    pub binary_schema: Option<SchemaVersion>,
    /// The schema-version range this driver understands.
    pub supported: crate::SchemaVersionRange,
    /// Required verbs the binary does not advertise.
    pub missing_verbs: Vec<String>,
}

impl Compatibility {
    /// True when the binary's schema is in range and every required
    /// verb is present.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        match self.binary_schema {
            Some(schema) => self.supported.contains(schema) && self.missing_verbs.is_empty(),
            None => false,
        }
    }
}

fn parse_version_info(schema: SchemaVersion, data: &Value) -> VersionInfo {
    let version = data
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("0.0.0")
        .to_owned();
    let git_describe = data
        .get("git_describe")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let verbs = data
        .get("verbs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    VersionInfo {
        version,
        schema_version: schema,
        git_describe,
        verbs,
    }
}

#[derive(Debug, serde::Deserialize)]
struct EnvelopeWire<T> {
    schema_version: u32,
    #[serde(default)]
    command: Option<String>,
    data: T,
    #[serde(default)]
    exit_code: i32,
    #[serde(default)]
    warnings: Vec<EnvelopeWarning>,
}

#[derive(serde::Deserialize)]
struct SchemaProbe {
    schema_version: u32,
}

// Read just `schema_version`, ignoring the typed payload, so a too-new
// envelope reads as a mismatch instead of a payload parse error. `None`
// when the bytes lack the field.
fn read_schema_version(stdout: &[u8]) -> Option<u32> {
    serde_json::from_slice::<SchemaProbe>(stdout)
        .ok()
        .map(|p| p.schema_version)
}

fn parse_envelope<T: DeserializeOwned>(
    verb: VerbId<'static>,
    outcome: &RawOutcome,
    executed: ExecutedInvocation,
) -> Result<Envelope<T>, DriverError> {
    // Check the ceiling before the strict parse: a too-new envelope
    // must read as a mismatch, not a `deny_unknown_fields` parse error.
    if let Some(sv) = read_schema_version(&outcome.stdout) {
        let schema = SchemaVersion::new(sv);
        if !crate::SUPPORTED_SCHEMA.contains(schema) {
            return Err(DriverError::SchemaVersionMismatch {
                got: schema,
                supported_min: crate::SUPPORTED_SCHEMA.min,
                supported_max: crate::SUPPORTED_SCHEMA.max,
            });
        }
    }
    let wire: EnvelopeWire<T> = serde_json::from_slice(&outcome.stdout).map_err(|source| {
        DriverError::UnparseableOutput {
            verb: verb.clone(),
            expected: "JSON envelope",
            stdout: outcome.stdout.clone(),
            stderr: outcome.stderr.clone(),
            source,
        }
    })?;
    let schema = SchemaVersion::new(wire.schema_version);
    let command = wire
        .command
        .as_deref()
        .and_then(|s| Verb::parse(s).ok())
        .or_else(|| verb.as_known())
        .unwrap_or(Verb::Version);
    Ok(Envelope::new(
        schema,
        command,
        wire.data,
        wire.exit_code,
        wire.warnings,
        executed,
    ))
}

#[derive(Debug, Clone, Copy)]
enum StdinSnapshot {
    None,
    Bytes(usize),
    Reader,
}
