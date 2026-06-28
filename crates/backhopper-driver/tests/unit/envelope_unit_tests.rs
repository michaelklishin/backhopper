// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;
use std::time::{Duration, SystemTime};

use backhopper_driver::{
    EnvOverlay, ExecutedInvocation, ExitClass, GlobalOptions, RawOutcome, SchemaVersion,
    StdinPayload, SubprocessBackend, Verb, VerbId,
};

fn dummy_executed() -> ExecutedInvocation {
    let outcome = RawOutcome::ok(b"".to_vec()).with_duration(Duration::from_millis(5));
    let stdin = StdinPayload::None;
    ExecutedInvocation::from_outcome(
        VerbId::Known(Verb::Version),
        &stdin,
        &outcome,
        SystemTime::UNIX_EPOCH,
    )
}

#[test]
fn from_outcome_records_byte_counts_and_argv() {
    let outcome = RawOutcome::ok(b"hello".to_vec())
        .with_stderr(b"warn".to_vec())
        .with_duration(Duration::from_millis(11))
        .with_argv(["a", "b"].into_iter().map(Into::into));
    let stdin = StdinPayload::Bytes(b"abcdef");
    let exec = ExecutedInvocation::from_outcome(
        VerbId::Known(Verb::CheckPatch),
        &stdin,
        &outcome,
        SystemTime::UNIX_EPOCH,
    );

    assert_eq!(exec.stdout_bytes, 5);
    assert_eq!(exec.stderr_bytes, 4);
    assert_eq!(exec.duration, Duration::from_millis(11));
    assert_eq!(exec.stdin_bytes, Some(6));
    assert_eq!(exec.exit_class, ExitClass::VerdictOutcome);
    assert_eq!(exec.argv.len(), 2);
}

#[test]
fn from_outcome_carries_no_stdin_count_for_none_or_reader() {
    let outcome = RawOutcome::ok(b"".to_vec());
    let stdin = StdinPayload::None;
    let exec = ExecutedInvocation::from_outcome(
        VerbId::Known(Verb::Version),
        &stdin,
        &outcome,
        SystemTime::UNIX_EPOCH,
    );
    assert_eq!(exec.stdin_bytes, None);
}

#[test]
fn subprocess_backend_carries_binary_path() {
    let backend = SubprocessBackend::new("/tmp/fake-backhopper");
    assert_eq!(backend.binary_path(), Path::new("/tmp/fake-backhopper"));
}

#[test]
fn global_options_default_does_not_inject_anything() {
    let opts = GlobalOptions::default();
    assert!(!opts.non_interactive);
    assert!(opts.timeout.is_none());
    assert!(opts.cancellation.is_none());
    assert!(opts.env.overrides.is_empty());
}

#[test]
fn env_overlay_with_overrides_appends_keys() {
    let overlay = EnvOverlay::inherit()
        .with("RABBITMQ_NODENAME", "rabbit@localhost")
        .with("RABBITMQ_BASE", "/var/lib/rabbitmq");
    assert_eq!(overlay.overrides.len(), 2);
}

#[test]
fn schema_version_serialises_transparently() {
    let v = SchemaVersion::new(7);
    let j = serde_json::to_string(&v).unwrap();
    assert_eq!(j, "7");
}

#[test]
fn dummy_executed_smoke() {
    let exec = dummy_executed();
    assert_eq!(exec.stdout_bytes, 0);
    assert_eq!(exec.exit_code, 0);
}
