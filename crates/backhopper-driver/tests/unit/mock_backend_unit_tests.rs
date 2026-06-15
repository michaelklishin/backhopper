// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::SeriesName;
use backhopper_driver::{Backhopper, DriverError, ErrorKind, ExplainFormat, Verb, VerbId};
use serde_json::{Value, json};

fn series(name: &str) -> SeriesName {
    name.parse().expect("valid series name fixture")
}

fn series_evaluation_fixture() -> Value {
    json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": [
                { "project": "ra", "tag": "v2.16.7" }
            ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "compatible" },
                    "tracked_refs": 0
                }
            ],
            "summary": {
                "compatible": 1,
                "requires_adaptation": 0,
                "incompatible": 0
            }
        },
        "diagnostics": {}
    })
}

#[test]
fn json_ok_response_round_trips_through_mock_backend() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckPatch),
            MockResponse::json_ok(series_evaluation_fixture()),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let evaluation = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"--- a\n+++ b\n".to_vec())
        .run()
        .expect("envelope parses");

    assert_eq!(evaluation.results.results.len(), 1);
    assert_eq!(evaluation.results.summary.compatible, 1);
}

#[test]
fn invocation_log_captures_every_call() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckPatch),
            MockResponse::json_ok(series_evaluation_fixture()),
        )
        .build();
    let recorded_handle = {
        let driver = Backhopper::with_backend(backend);
        let _ = driver
            .check()
            .patch()
            .series(series("rabbitmq-4.2"))
            .patch_bytes(b"--- a\n+++ b\n".to_vec())
            .explain(ExplainFormat::Markdown)
            .run()
            .unwrap();
        driver.backend().invocations()
    };

    assert_eq!(recorded_handle.len(), 1);
    let invocation = &recorded_handle[0];
    assert!(matches!(invocation.verb, VerbId::Known(Verb::CheckPatch)));
    assert!(
        invocation
            .args
            .iter()
            .any(|a| a.to_string_lossy() == "--series")
    );
    assert!(
        invocation
            .args
            .iter()
            .any(|a| a.to_string_lossy() == "--explain")
    );
}

#[test]
fn fallthrough_error_reports_unmatched_verb_via_transport() {
    let backend = MockBackend::builder()
        .fallthrough(Fallthrough::Error)
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .run()
        .expect_err("unmatched mock invocation should error");
    assert_eq!(err.kind(), ErrorKind::Transport);
}

#[test]
fn delayed_then_observes_the_wall_clock() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::DelayedThen {
                duration: Duration::from_millis(40),
                then: Box::new(MockResponse::json_ok(json!({"version": "0.1.0"}))),
            },
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let started = Instant::now();
    let _ = driver.version().expect("returns");
    assert!(started.elapsed() >= Duration::from_millis(30));
}

#[test]
fn json_with_code_routes_verdict_codes_through_envelope_path() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckPatch),
            MockResponse::json_with_code(series_evaluation_fixture(), 3),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let evaluation = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .run()
        .expect("verdict exit codes parse normally");
    assert_eq!(evaluation.results.summary.compatible, 1);
}

#[test]
fn tool_error_response_surfaces_as_tool_error() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckPatch),
            MockResponse::tool_error("config missing", 66),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .run()
        .expect_err("66 is not success-shaped");
    assert_eq!(err.kind(), ErrorKind::ToolError);
    // the failure path carries the same exact-rerun argv as success
    match &err {
        DriverError::ToolError {
            exit_code,
            stderr,
            executed,
            ..
        } => {
            assert_eq!(*exit_code, 66);
            assert_eq!(stderr, "config missing");
            assert!(executed.argv.iter().any(|a| a == "check"));
            assert!(executed.argv.iter().any(|a| a == "patch"));
        }
        other => panic!("expected ToolError, got {other:?}"),
    }
    let shown = err.to_string();
    assert!(shown.contains("check patch"));
    assert!(shown.contains("config missing"));
}

#[test]
fn verb_where_filters_by_predicate() {
    let evaluation = Arc::new(series_evaluation_fixture());
    let mine = Arc::clone(&evaluation);
    let backend = MockBackend::builder()
        .respond_with(
            MockMatcher::VerbWhere {
                verb: Verb::CheckPatch,
                predicate: Arc::new(|inv| {
                    inv.args.iter().any(|a| a.to_string_lossy() == "--explain")
                }),
            },
            move |_inv| MockResponse::json_ok((*mine).clone()),
        )
        .fallthrough(Fallthrough::Error)
        .build();
    let driver = Backhopper::with_backend(backend);

    let with_explain = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .explain(ExplainFormat::Markdown)
        .run();
    assert!(with_explain.is_ok());

    let without_explain = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .run();
    assert!(without_explain.is_err());

    let _ = evaluation;
}

#[test]
fn fallthrough_empty_returns_a_blank_outcome() {
    let backend = MockBackend::builder()
        .fallthrough(Fallthrough::Empty)
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .version()
        .expect_err("blank stdout fails to parse as envelope");
    assert_eq!(err.kind(), ErrorKind::UnparseableOutput);
}

#[test]
fn mock_response_error_is_propagated_verbatim() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::Version), |_inv| {
            MockResponse::Error(DriverError::Cancelled {
                verb: VerbId::Known(Verb::Version),
                argv: Vec::new(),
            })
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver.version().expect_err("error response surfaces");
    assert_eq!(err.kind(), ErrorKind::Cancelled);
}

#[test]
fn clear_log_resets_recorded_invocations() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::json_ok(json!({"version": "0.1.0"})),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let _ = driver.version().unwrap();
    assert_eq!(driver.backend().invocations().len(), 1);
    driver.backend().clear_log();
    assert_eq!(driver.backend().invocations().len(), 0);
}
