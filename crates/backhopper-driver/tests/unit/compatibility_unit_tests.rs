// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use backhopper_driver::mock::{MockBackend, MockMatcher, MockResponse};
use backhopper_driver::{Backhopper, ErrorKind, Verb};
use serde_json::json;

fn version_envelope(schema: u64, version: &str, verbs: &[&str]) -> Vec<u8> {
    let env = json!({
        "schema_version": schema,
        "command": "version",
        "data": { "version": version, "verbs": verbs },
        "exit_code": 0
    });
    serde_json::to_vec(&env).expect("envelope serialises")
}

fn driver_returning(stdout: Vec<u8>) -> Backhopper<MockBackend> {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::raw_ok(stdout),
        )
        .build();
    Backhopper::with_backend(backend)
}

// invoke_raw does not inject --formatter json, so compatibility must request it itself
#[test]
fn compatibility_requests_json_formatter() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::Version), |inv| {
            let args: Vec<String> = inv
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--formatter" && w[1] == "json"),
                "compatibility must request json: {args:?}"
            );
            MockResponse::raw_ok(version_envelope(1, "0.16.0", &[]))
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let report = driver.compatibility(&[]).expect("probe succeeds");
    assert!(report.is_compatible());
}

// the schema-first read makes a too-new binary fail as a mismatch, not a parse error
#[test]
fn out_of_range_schema_is_a_mismatch_not_unparseable() {
    let driver = driver_returning(version_envelope(9999, "9.9.9", &[]));
    let err = driver.version().expect_err("out-of-range schema");
    assert_eq!(err.kind(), ErrorKind::SchemaVersionMismatch);
}

#[test]
fn compatibility_reports_out_of_range_without_erroring() {
    let driver = driver_returning(version_envelope(9999, "9.9.9", &["check batch"]));
    let report = driver.compatibility(&[]).expect("probe succeeds");
    assert!(!report.is_compatible());
    assert_eq!(report.binary_schema.map(|s| s.get()), Some(9999));
    assert_eq!(report.binary_version, "9.9.9");
}

#[test]
fn compatibility_in_range_with_required_verbs_is_compatible() {
    let driver = driver_returning(version_envelope(
        1,
        "0.16.0",
        &["check batch", "suites plan"],
    ));
    let report = driver
        .compatibility(&["check batch", "suites plan"])
        .expect("probe succeeds");
    assert!(report.is_compatible());
    assert_eq!(report.binary_version, "0.16.0");
    assert!(report.missing_verbs.is_empty());
}

#[test]
fn compatibility_lists_missing_required_verbs() {
    let driver = driver_returning(version_envelope(1, "0.16.0", &["check batch"]));
    let report = driver
        .compatibility(&["check batch", "suites plan"])
        .expect("probe succeeds");
    assert_eq!(report.missing_verbs, vec!["suites plan".to_string()]);
    assert!(!report.is_compatible());
}

// a version envelope that predates schema_version sits below the floor the lenient probe can detect
#[test]
fn compatibility_without_schema_field_is_below_floor() {
    let env = json!({ "command": "version", "data": { "version": "0.0.1", "verbs": [] } });
    let driver = driver_returning(serde_json::to_vec(&env).unwrap());
    let report = driver.compatibility(&[]).expect("probe succeeds");
    assert!(report.binary_schema.is_none());
    assert!(!report.is_compatible());
}

#[test]
fn version_info_missing_verbs_filters_to_the_absent() {
    let driver = driver_returning(version_envelope(
        1,
        "0.16.0",
        &["check batch", "snapshots diff"],
    ));
    let info = driver.version().expect("version parses");
    assert!(info.has_verb("check batch"));
    assert_eq!(
        info.missing_verbs(&["check batch", "suites plan"]),
        vec!["suites plan".to_string()]
    );
}
