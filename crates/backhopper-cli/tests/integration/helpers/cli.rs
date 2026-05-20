// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper` invocation helpers shared across integration tests.
//!
//! Modelled after `rabbitmqadmin-rs`'s `test_helpers`: small wrappers
//! around `assert_cmd::Command::cargo_bin("backhopper")` that take an
//! `args` iterator and return either an `Assert` or a captured stdout
//! string.

#![allow(dead_code)]

use std::ffi::OsStr;

use assert_cmd::Command;
use assert_cmd::assert::Assert;

/// Run `backhopper <args>` and return the raw `Assert`.
pub(crate) fn run<I, S>(args: I) -> Assert
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(args)
        .assert()
}

/// Run with stdin attached and return the raw `Assert`.
pub(crate) fn run_with_stdin<I, S>(args: I, stdin: &str) -> Assert
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(args)
        .write_stdin(stdin.to_owned())
        .assert()
}

/// Run and assert exit code 0.
pub(crate) fn run_succeeds<I, S>(args: I) -> Assert
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run(args).success()
}

/// Run and assert a non-zero exit code.
pub(crate) fn run_fails<I, S>(args: I) -> Assert
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run(args).failure()
}

/// Captured stdout from an `Assert`.
pub(crate) fn stdout(a: &Assert) -> String {
    String::from_utf8(a.get_output().stdout.clone()).expect("stdout is utf-8")
}

/// Captured stderr from an `Assert`.
pub(crate) fn stderr(a: &Assert) -> String {
    String::from_utf8(a.get_output().stderr.clone()).expect("stderr is utf-8")
}
