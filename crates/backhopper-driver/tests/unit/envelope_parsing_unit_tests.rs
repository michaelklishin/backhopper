// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::SeriesName;
use backhopper_driver::{
    Backhopper, ErrorKind, RawOutcome, SchemaVersion, StdinPayload, Verb, VerbId,
};
use serde_json::{Value, json};

fn series(name: &str) -> SeriesName {
    name.parse().expect("valid series name fixture")
}

#[test]
fn typed_invoke_returns_envelope_with_argv_and_executed_snapshot() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::json_ok(json!({"version": "1.2.3"})),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let env = driver
        .invoke::<Value, &str>(VerbId::Known(Verb::Version), &[], StdinPayload::None)
        .expect("envelope parses");

    assert_eq!(env.schema_version(), SchemaVersion::new(1));
    assert!(!env.argv().is_empty());
    assert_eq!(env.executed().exit_code, 0);
    assert_eq!(
        env.data().get("version").and_then(Value::as_str),
        Some("1.2.3")
    );
}

#[test]
fn version_probe_is_cached_after_first_call() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&call_count);
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::Version), move |_inv| {
            count_clone.fetch_add(1, Ordering::SeqCst);
            MockResponse::json_ok(json!({"version": "0.1.0"}))
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let _ = driver.version().expect("first probe");
    let _ = driver.version().expect("second probe");
    let _ = driver.version().expect("third probe");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "version probe must be cached"
    );
}

#[test]
fn schema_version_mismatch_surfaces_on_parse() {
    let envelope = json!({
        "schema_version": 999,
        "command": "version",
        "data": {"version": "1.0.0"},
        "exit_code": 0
    });
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::Raw(RawOutcome::ok(bytes)),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .version()
        .expect_err("unsupported schema is rejected");
    assert_eq!(err.kind(), ErrorKind::SchemaVersionMismatch);
}

#[test]
fn invalid_json_routes_to_unparseable_output() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::Raw(RawOutcome::ok(b"not json at all".to_vec())),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver.version().expect_err("garbage does not parse");
    assert_eq!(err.kind(), ErrorKind::UnparseableOutput);
}

#[test]
fn invoke_raw_round_trips_arbitrary_verb_id() {
    let backend = MockBackend::builder()
        .respond_with(
            MockMatcher::Custom(Arc::new(|inv| {
                inv.verb.display_name() == "future experiment"
            })),
            |_inv| MockResponse::raw_ok(b"hello".to_vec()),
        )
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = Backhopper::with_backend(backend);
    let outcome = driver
        .invoke_raw(
            VerbId::Custom(Cow::Owned(vec![
                Cow::Borrowed("future"),
                Cow::Borrowed("experiment"),
            ])),
            &[] as &[&str],
            StdinPayload::None,
        )
        .expect("invoke_raw");
    assert_eq!(outcome.stdout, b"hello".to_vec());
    assert!(!outcome.argv.is_empty());
}

#[test]
fn malformed_evaluation_payload_routes_to_unparseable_output() {
    let envelope = json!({
        "schema_version": 1,
        "command": "check patch",
        "data": { "queried_against": "not an object" },
        "exit_code": 0
    });
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckPatch),
            MockResponse::Raw(RawOutcome::ok(bytes)),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .run()
        .expect_err("malformed payload should not panic");
    assert_eq!(err.kind(), ErrorKind::UnparseableOutput);
}

#[test]
fn envelope_warnings_and_exit_code_round_trip() {
    let envelope = json!({
        "schema_version": 1,
        "command": "version",
        "data": {"version": "0.1.0"},
        "exit_code": 0,
        "warnings": [
            {"code": "stale-extractor", "message": "snapshot extractor 0 vs binary 1"}
        ]
    });
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::Raw(RawOutcome::ok(bytes)),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let env = driver
        .invoke::<Value, &str>(VerbId::Known(Verb::Version), &[], StdinPayload::None)
        .expect("envelope parses");
    assert_eq!(env.warnings().len(), 1);
    assert_eq!(env.warnings()[0].code, "stale-extractor");
    assert_eq!(env.exit_code(), 0);
    assert_eq!(env.command(), Some(Verb::Version));
}

#[test]
fn envelope_map_preserves_metadata() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::json_ok(json!({"version": "1.0.0"})),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let env = driver
        .invoke::<Value, &str>(VerbId::Known(Verb::Version), &[], StdinPayload::None)
        .unwrap();
    let argv_before = env.argv().to_vec();
    let schema_before = env.schema_version();
    let mapped = env.map(|v| v.get("version").and_then(Value::as_str).map(str::to_owned));
    assert_eq!(mapped.argv(), argv_before.as_slice());
    assert_eq!(mapped.schema_version(), schema_before);
    assert_eq!(mapped.into_data().as_deref(), Some("1.0.0"));
}
