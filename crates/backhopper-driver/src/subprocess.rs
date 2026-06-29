// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Real subprocess backend.
//!
//! Lifecycle guarantees the [`SubprocessBackend`] holds:
//!
//! * On caller panic or early return between spawn and wait, the
//!   child is killed via a `Drop` guard on the [`std::process::Child`]
//! * Cancellation: terminates the child after a short grace period
//!   (currently `kill` directly because portable signalling without
//!   `nix` is limited; the grace period matters once a signal-based
//!   path is wired in)
//! * Timeout: same termination path
//! * The argv returned in [`crate::backend::RawOutcome::argv`]
//!   includes the program path at index 0, then the verb path, then
//!   the user's arguments, matching `Command::get_program` plus
//!   `get_args`
//!
//! The backend ignores `SIGPIPE` propagation concerns: stdin is
//! always fully written before `wait` is called, so a closed pipe
//! produces an `io::Error` rather than a signal kill of the parent.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::backend::{Backend, Invocation, OutputChannel, OutputPolicy, RawOutcome, assemble_argv};
use crate::cancellation::CancellationToken;
use crate::error::DriverError;
use crate::options::{EnvInheritance, EnvOverlay};
use crate::stdin::StdinPayload;
use crate::verb::VerbId;

/// Default subprocess backend.
#[derive(Debug, Clone)]
pub struct SubprocessBackend {
    binary_path: PathBuf,
    poll_interval: Duration,
}

impl SubprocessBackend {
    /// Construct a backend that spawns the binary at `binary_path`.
    pub fn new(binary_path: impl Into<PathBuf>) -> Self {
        Self {
            binary_path: binary_path.into(),
            poll_interval: Duration::from_millis(25),
        }
    }

    /// Override the poll interval used when waiting on timeout or
    /// cancellation. Shorter intervals reduce shutdown latency at
    /// the cost of more wakeups per invocation.
    pub fn with_poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    /// The configured binary path.
    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }
}

impl Backend for SubprocessBackend {
    fn invoke(&self, invocation: Invocation<'_>) -> Result<RawOutcome, DriverError> {
        let argv = assemble_argv(
            self.binary_path.as_os_str(),
            &invocation.verb,
            &invocation.args,
        );
        let mut command = Command::new(&self.binary_path);
        let verb_path = invocation.verb.cli_path();
        for seg in &verb_path {
            command.arg(seg);
        }
        for arg in invocation.args.iter() {
            let arg_ref: &OsStr = arg;
            command.arg(arg_ref);
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if let Some(cwd) = invocation.working_directory {
            command.current_dir(cwd);
        }
        apply_env(&mut command, invocation.env);

        let started = Instant::now();
        let child = command.spawn().map_err(|source| DriverError::Spawn {
            binary_path: self.binary_path.clone(),
            source,
        })?;
        let guard = ChildGuard::new(child);

        run_invocation(
            guard,
            invocation,
            started,
            argv,
            self.poll_interval,
            self.binary_path.clone(),
        )
    }
}

fn apply_env(command: &mut Command, overlay: &EnvOverlay) {
    match &overlay.inheritance {
        EnvInheritance::Inherit => {}
        EnvInheritance::Clear => {
            command.env_clear();
        }
        EnvInheritance::AllowList(allowed) => {
            command.env_clear();
            for key in *allowed {
                if let Some(value) = env::var_os(key) {
                    command.env(key, value);
                }
            }
        }
    }
    for (k, v) in &overlay.overrides {
        command.env(k, v);
    }
}

fn run_invocation(
    mut guard: ChildGuard,
    invocation: Invocation<'_>,
    started: Instant,
    argv: Vec<OsString>,
    poll_interval: Duration,
    binary_path: PathBuf,
) -> Result<RawOutcome, DriverError> {
    let mut stdin_handle = guard.child.stdin.take();
    let stdout_handle = guard.child.stdout.take();
    let stderr_handle = guard.child.stderr.take();

    let stdout_cap = capacity_for(invocation.stdout_policy);
    let stderr_cap = capacity_for(invocation.stderr_policy);

    let stdout_overflow = Arc::new(AtomicBool::new(false));
    let stderr_overflow = Arc::new(AtomicBool::new(false));
    // drain stdout and stderr before writing stdin: a child that fills its
    // output pipe while we are still feeding a large stdin would otherwise
    // deadlock, with both sides blocked on a full pipe
    let stdout_thread =
        stdout_handle.map(|h| spawn_reader(h, stdout_cap, Arc::clone(&stdout_overflow)));
    let stderr_thread =
        stderr_handle.map(|h| spawn_reader(h, stderr_cap, Arc::clone(&stderr_overflow)));

    let write_result = write_stdin(stdin_handle.as_mut(), invocation.stdin);
    drop(stdin_handle);
    write_result.map_err(|source| DriverError::Spawn {
        binary_path: binary_path.clone(),
        source,
    })?;

    let status = wait_with_policy(
        &mut guard,
        WaitPolicy {
            binary_path: &binary_path,
            timeout: invocation.timeout,
            cancellation: invocation.cancellation,
            poll_interval,
            started,
            verb: invocation.verb.clone().into_owned(),
            argv: argv.clone(),
        },
    )?;

    let stdout = join_reader(stdout_thread, &invocation.verb, OutputChannel::Stdout)?;
    let stderr = join_reader(stderr_thread, &invocation.verb, OutputChannel::Stderr)?;

    if stdout_overflow.load(Ordering::Acquire) {
        return Err(DriverError::OutputTooLarge {
            verb: invocation.verb.clone().into_owned(),
            channel: OutputChannel::Stdout,
            limit: stdout_cap.unwrap_or(usize::MAX),
        });
    }
    if stderr_overflow.load(Ordering::Acquire) {
        return Err(DriverError::OutputTooLarge {
            verb: invocation.verb.clone().into_owned(),
            channel: OutputChannel::Stderr,
            limit: stderr_cap.unwrap_or(usize::MAX),
        });
    }

    guard.disarm();

    let exit_code = status.code().unwrap_or(-1);
    Ok(RawOutcome {
        exit_code,
        stdout,
        stderr,
        duration: started.elapsed(),
        argv,
    })
}

// a panicked capture thread must not read as empty output
fn join_reader(
    thread: Option<JoinHandle<Vec<u8>>>,
    verb: &VerbId<'_>,
    channel: OutputChannel,
) -> Result<Vec<u8>, DriverError> {
    match thread {
        None => Ok(Vec::new()),
        Some(t) => t.join().map_err(|_| DriverError::ReaderPanicked {
            verb: verb.clone().into_owned(),
            channel,
        }),
    }
}

fn write_stdin(handle: Option<&mut ChildStdin>, payload: StdinPayload<'_>) -> io::Result<()> {
    let Some(handle) = handle else {
        return Ok(());
    };
    match payload {
        StdinPayload::None => Ok(()),
        StdinPayload::Bytes(b) => handle.write_all(b),
        StdinPayload::Reader(r) => io::copy(r, handle).map(|_| ()),
    }
}

fn capacity_for(policy: OutputPolicy) -> Option<usize> {
    match policy {
        OutputPolicy::Buffer => None,
        OutputPolicy::BufferUpTo(cap) => Some(cap),
        OutputPolicy::Discard => Some(0),
    }
}

fn spawn_reader(
    mut handle: impl Read + Send + 'static,
    cap: Option<usize>,
    overflow: Arc<AtomicBool>,
) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut scratch = [0u8; 8192];
        loop {
            match handle.read(&mut scratch) {
                Ok(0) => break,
                Ok(n) => match cap {
                    Some(0) => {}
                    Some(limit) if buf.len().saturating_add(n) > limit => {
                        overflow.store(true, Ordering::Release);
                        break;
                    }
                    _ => buf.extend_from_slice(&scratch[..n]),
                },
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        buf
    })
}

// Read-only wait-loop inputs, bundled to keep the signature small.
// `verb` and `argv` are owned so the error paths need not borrow the
// argv the success path moves into `RawOutcome`.
struct WaitPolicy<'a> {
    binary_path: &'a Path,
    timeout: Option<Duration>,
    cancellation: Option<&'a CancellationToken>,
    poll_interval: Duration,
    started: Instant,
    verb: VerbId<'static>,
    argv: Vec<OsString>,
}

fn wait_with_policy(
    guard: &mut ChildGuard,
    policy: WaitPolicy<'_>,
) -> Result<ExitStatus, DriverError> {
    loop {
        if let Some(token) = policy.cancellation {
            if token.is_cancelled() {
                guard.kill_now();
                let _ = guard.child.wait();
                return Err(DriverError::Cancelled {
                    verb: policy.verb.clone(),
                    argv: policy.argv.clone(),
                });
            }
        }
        if let Some(limit) = policy.timeout {
            if policy.started.elapsed() >= limit {
                guard.kill_now();
                let _ = guard.child.wait();
                return Err(DriverError::Timeout {
                    verb: policy.verb.clone(),
                    after: limit,
                    argv: policy.argv.clone(),
                });
            }
        }
        match guard.child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(policy.poll_interval),
            Err(source) => {
                return Err(DriverError::Spawn {
                    binary_path: policy.binary_path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

/// RAII guard that kills the child if dropped before `disarm()`.
struct ChildGuard {
    child: Child,
    armed: bool,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn kill_now(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Search `$BACKHOPPER_BINARY_PATH` then `$PATH` for the binary.
///
/// Used by `Backhopper::auto_discover`; lives here because the
/// search is subprocess-specific.
pub(crate) fn discover_binary() -> Result<PathBuf, DriverError> {
    let mut searched = Vec::new();
    if let Some(explicit) = env::var_os("BACKHOPPER_BINARY_PATH") {
        let path = PathBuf::from(&explicit);
        searched.push(path.clone());
        if is_executable(&path) {
            return Ok(path);
        }
    }
    if let Some(path_env) = env::var_os("PATH") {
        for entry in env::split_paths(&path_env) {
            let candidate = entry.join(binary_basename());
            searched.push(candidate.clone());
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    let last_err = io::Error::new(io::ErrorKind::NotFound, "backhopper not on PATH");
    Err(DriverError::BinaryNotFound {
        searched,
        source: last_err,
    })
}

fn is_executable(p: &Path) -> bool {
    fs::metadata(p).map(|m| m.is_file()).unwrap_or(false)
}

#[cfg(windows)]
fn binary_basename() -> &'static OsStr {
    OsStr::new("backhopper.exe")
}

#[cfg(not(windows))]
fn binary_basename() -> &'static OsStr {
    OsStr::new("backhopper")
}
