// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::time::Duration;

use backhopper_driver::{
    EnvOverlay, Invocation, OutputPolicy, RawOutcome, StdinPayload, Verb, VerbId,
};

#[test]
fn invocation_new_defaults_are_sensible() {
    let env = EnvOverlay::default();
    let inv = Invocation::new(VerbId::Known(Verb::Version), &env);
    assert!(inv.args.is_empty());
    assert!(matches!(inv.stdin, StdinPayload::None));
    assert!(matches!(inv.stdout_policy, OutputPolicy::BufferUpTo(_)));
    assert!(inv.timeout.is_none());
    assert!(inv.cancellation.is_none());
}

#[test]
fn invocation_with_args_replaces_the_slot() {
    let env = EnvOverlay::default();
    let args: Vec<Cow<'_, OsStr>> = vec![Cow::Borrowed(OsStr::new("--quiet"))];
    let inv = Invocation::new(VerbId::Known(Verb::Doctor), &env).with_args(Cow::Owned(args));
    assert_eq!(inv.args.len(), 1);
}

#[test]
fn output_policy_default_is_a_buffer_cap() {
    match OutputPolicy::default() {
        OutputPolicy::BufferUpTo(n) => assert!(n > 0),
        other => panic!("default should be BufferUpTo, got {other:?}"),
    }
}

#[test]
fn raw_outcome_builders_compose() {
    let outcome = RawOutcome::ok(b"hi".to_vec())
        .with_exit_code(2)
        .with_stderr(b"warn".to_vec())
        .with_duration(Duration::from_millis(7))
        .with_argv(["a"].into_iter().map(Into::into));
    assert_eq!(outcome.exit_code, 2);
    assert_eq!(outcome.argv, vec![OsString::from("a")]);
}
