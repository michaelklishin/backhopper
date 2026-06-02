// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Real `SubprocessBackend` exercised against shell stubs in a `TempDir`.
//! Skipped on Windows because the stubs use `/bin/sh`.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use backhopper_driver::{
    Backend, Backhopper, CancellationToken, EnvOverlay, ErrorKind, Invocation, OutputPolicy,
    SubprocessBackend, Verb, VerbId,
};
use tempfile::TempDir;

fn write_stub(dir: &TempDir, body: &str) -> PathBuf {
    let path = dir.path().join("backhopper");
    let mut f = fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    f.write_all(body.as_bytes()).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

#[test]
fn invoke_parses_a_well_formed_envelope() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(
        &dir,
        r#"echo '{"schema_version":1,"command":"version","data":{"version":"0.1.0"},"exit_code":0}'
exit 0
"#,
    );
    let driver = Backhopper::with_binary_path(stub);
    let info = driver.version().expect("version probes");
    assert_eq!(info.version, "0.1.0");
}

#[test]
fn non_success_shaped_exit_routes_to_tool_error() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(
        &dir,
        r#"echo "config missing" 1>&2
exit 66
"#,
    );
    let driver = Backhopper::with_binary_path(stub);
    let err = driver.version().expect_err("66 is not success-shaped");
    assert_eq!(err.kind(), ErrorKind::ToolError);
}

#[test]
fn timeout_kills_the_child() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(&dir, "sleep 5\nexit 0\n");
    let mut driver = Backhopper::with_binary_path(stub);
    driver.options_mut().timeout = Some(Duration::from_millis(200));
    let err = driver.version().expect_err("verb should time out");
    assert_eq!(err.kind(), ErrorKind::Timeout);
}

#[test]
fn cancellation_kills_the_child() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(&dir, "sleep 5\nexit 0\n");
    let (token, aborter) = CancellationToken::new();
    let mut driver = Backhopper::with_binary_path(stub);
    driver.options_mut().cancellation = Some(token);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        aborter.abort();
    });
    let err = driver.version().expect_err("verb should be cancelled");
    assert_eq!(err.kind(), ErrorKind::Cancelled);
}

#[test]
fn output_too_large_when_cap_is_exceeded() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(&dir, "head -c 4096 < /dev/zero | tr '\\0' 'A'\nexit 0\n");
    let backend = SubprocessBackend::new(stub);
    let env = EnvOverlay::default();
    let mut invocation = Invocation::new(VerbId::Known(Verb::Version), &env);
    invocation.stdout_policy = OutputPolicy::BufferUpTo(256);
    let result = backend.invoke(invocation);
    match result {
        Err(backhopper_driver::DriverError::OutputTooLarge { .. }) => {}
        Err(other) => panic!("expected OutputTooLarge, got {other}"),
        Ok(o) => panic!("unexpected success, captured {} bytes", o.stdout.len()),
    }
}

#[test]
fn env_overlay_overrides_reach_the_child() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(
        &dir,
        r#"echo "{\"schema_version\":1,\"command\":\"version\",\"data\":{\"version\":\"$BACKHOPPER_TEST_VALUE\"},\"exit_code\":0}"
exit 0
"#,
    );
    let mut driver = Backhopper::with_binary_path(stub);
    driver.options_mut().env = driver
        .options()
        .env
        .clone()
        .with("BACKHOPPER_TEST_VALUE", "9.9.9");
    let info = driver.version().expect("version probes");
    assert_eq!(info.version, "9.9.9");
}

#[test]
fn unparseable_output_surfaces_structured_error() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(
        &dir,
        r#"echo "not json"
exit 0
"#,
    );
    let driver = Backhopper::with_binary_path(stub);
    let err = driver.version().expect_err("garbage does not parse");
    assert_eq!(err.kind(), ErrorKind::UnparseableOutput);
}

#[test]
fn spawn_fails_for_nonexistent_binary_path() {
    let driver = Backhopper::with_binary_path("/this/path/should/not/exist/backhopper-xyz");
    let err = driver.version().expect_err("missing binary cannot spawn");
    assert_eq!(err.kind(), ErrorKind::Spawn);
}

#[test]
fn working_directory_is_honoured_by_the_child() {
    let dir = TempDir::new().unwrap();
    let stub = write_stub(
        &dir,
        r#"printf '{"schema_version":1,"command":"version","data":{"version":"%s"},"exit_code":0}\n' "$(basename $PWD)"
exit 0
"#,
    );
    let workdir = TempDir::new().unwrap();
    let workdir_name = workdir
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let mut driver = Backhopper::with_binary_path(stub);
    driver.options_mut().working_directory = Some(workdir.path().to_path_buf());
    let info = driver.version().expect("version probes");
    assert_eq!(info.version, workdir_name);
}
