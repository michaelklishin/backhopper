// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::sync::{Arc, Mutex};

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::{Backhopper, GlobalOptions, StdinPayload, Verb, VerbId};
use serde_json::{Value, json};

fn capture_argv() -> (MockBackend, Arc<Mutex<Vec<String>>>) {
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let writer = Arc::clone(&captured);
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::Version), move |inv| {
            let args = inv
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            *writer.lock().unwrap() = args;
            MockResponse::json_ok(json!({"version": "0"}))
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    (backend, captured)
}

#[test]
fn quiet_flag_is_forwarded_when_set() {
    let (backend, captured) = capture_argv();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().quiet = true;
    let _ = driver.version().unwrap();
    assert!(captured.lock().unwrap().iter().any(|a| a == "--quiet"));
}

#[test]
fn dry_run_flag_is_forwarded_when_set() {
    let (backend, captured) = capture_argv();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().dry_run = true;
    let _ = driver.version().unwrap();
    assert!(captured.lock().unwrap().iter().any(|a| a == "--dry-run"));
}

#[test]
fn config_path_and_snapshot_dir_are_forwarded_with_value() {
    let (backend, captured) = capture_argv();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().config_path = Some("/etc/backhopper.toml".into());
    driver.options_mut().snapshot_dir = Some("/var/lib/snap".into());
    let _ = driver.version().unwrap();
    let args = captured.lock().unwrap().clone();
    let pairs: Vec<(&String, &String)> = args.windows(2).map(|w| (&w[0], &w[1])).collect();
    assert!(
        pairs.iter().any(
            |(k, v)| k.as_str() == "--config-file-path" && v.as_str() == "/etc/backhopper.toml"
        )
    );
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k.as_str() == "--snapshot-dir-path" && v.as_str() == "/var/lib/snap")
    );
}

#[test]
fn repo_dir_path_is_forwarded_with_value() {
    let (backend, captured) = capture_argv();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some("/home/user/proj.git".into());
    let _ = driver.version().unwrap();
    let args = captured.lock().unwrap().clone();
    let pairs: Vec<(&String, &String)> = args.windows(2).map(|w| (&w[0], &w[1])).collect();
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k.as_str() == "--repo-dir-path" && v.as_str() == "/home/user/proj.git"),
        "expected --repo-dir-path in args: {args:?}",
    );
}

#[test]
fn non_interactive_is_on_by_default() {
    let (backend, captured) = capture_argv();
    let driver = Backhopper::with_backend(backend);
    let _ = driver.version().unwrap();
    assert!(
        captured
            .lock()
            .unwrap()
            .iter()
            .any(|a| a == "--non-interactive")
    );
}

#[test]
fn formatter_json_is_injected_for_typed_invoke() {
    let (backend, captured) = capture_argv();
    let driver = Backhopper::with_backend(backend);
    let _ = driver
        .invoke::<Value, &str>(VerbId::Known(Verb::Version), &[], StdinPayload::None)
        .unwrap();
    let args = captured.lock().unwrap().clone();
    assert!(
        args.windows(2)
            .any(|w| w[0] == "--formatter" && w[1] == "json")
    );
}

#[test]
fn invoke_raw_does_not_force_formatter() {
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let writer = Arc::clone(&captured);
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Custom(Arc::new(|_| true)), move |inv| {
            *writer.lock().unwrap() = inv
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            MockResponse::raw_ok(b"".to_vec())
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let _ = driver
        .invoke_raw(
            VerbId::Known(Verb::Version),
            &[] as &[&str],
            StdinPayload::None,
        )
        .unwrap();
    let args = captured.lock().unwrap().clone();
    assert!(!args.iter().any(|a| a == "--formatter"));
}

#[test]
fn with_options_replaces_the_whole_struct() {
    let (backend, captured) = capture_argv();
    let opts = GlobalOptions::builder().quiet(true).verbose(false).build();
    let driver = Backhopper::with_backend(backend).with_options(opts);
    let _ = driver.version().unwrap();
    let args = captured.lock().unwrap().clone();
    assert!(args.iter().any(|a| a == "--quiet"));
    assert!(!args.iter().any(|a| a == "--non-interactive"));
}
