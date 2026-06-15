// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::PathBuf;

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::{CommitSha, SeriesName};
use backhopper_driver::{
    AggregateVerdict, Backhopper, ErrorKind, ExplainFormat, RawOutcome, StdinPayload, Verb,
};
use serde_json::{Value, json};

// Batch is repo-operating, so its driver needs a source repo dir.
fn repo_driver(backend: MockBackend) -> Backhopper<MockBackend> {
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some(PathBuf::from("/repo"));
    driver
}

fn series(name: &str) -> SeriesName {
    name.parse().expect("valid series name fixture")
}

fn commit_sha(hex: &str) -> CommitSha {
    hex.parse().expect("valid commit SHA fixture")
}

fn batch_payload_fixture() -> Value {
    json!({
        "queried_against": [{ "series": "rabbitmq-4.2", "pins": [] }],
        "results": [{
            "commit": "a".repeat(40),
            "series": "rabbitmq-4.2",
            "verdict": {
                "results": [],
                "summary": { "compatible": 0, "requires_adaptation": 0, "incompatible": 0 }
            }
        }]
    })
}

fn batch_payload_with_finding() -> Value {
    json!({
        "queried_against": [{
            "series": "rabbitmq-4.2",
            "pins": [{ "project": "ra", "tag": "v2.16.7" }]
        }],
        "results": [{
            "commit": "a".repeat(40),
            "series": "rabbitmq-4.2",
            "verdict": {
                "results": [{
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "requires_adaptation", "reasons": [{
                        "kind": "test_module_symbol_missing",
                        "suite_path": "deps/rabbit/test/x_SUITE.erl",
                        "missing_module": "amqp_utils",
                        "call_sites": [{ "function": "connection_config", "arity": 1, "line": 42 }]
                    }] }
                }],
                "summary": { "compatible": 0, "requires_adaptation": 1, "incompatible": 0 }
            },
            "diagnostics": {
                "missing_test_modules": { "deps/rabbit/test/x_SUITE.erl": { "amqp_utils": 3 } }
            }
        }]
    })
}

fn args_of(inv_args: &[Cow<'_, OsStr>]) -> Vec<String> {
    inv_args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn batch_builder_assembles_series_explain_target_and_stdin_commits() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckBatch), |inv| {
            let args = args_of(&inv.args);
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--series" && w[1] == "rabbitmq-4.2")
            );
            assert!(args.iter().any(|a| a == "--explain"));
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--target-repo-dir-path" && w[1] == "/tmp/target")
            );
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--target-ref" && w[1] == "v4.2.x")
            );
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--commits-file-path" && w[1] == "-")
            );
            MockResponse::json_ok(batch_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    let payload = driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .target_repo_dir_path("/tmp/target")
        .target_ref("v4.2.x")
        .explain(ExplainFormat::Text)
        .commits([commit_sha(&"a".repeat(40)), commit_sha(&"b".repeat(40))])
        .run()
        .expect("batch payload parses");
    assert_eq!(payload.results.len(), 1);
}

#[test]
fn batch_builder_multi_series_emits_one_flag_per_series() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckBatch), |inv| {
            let args = args_of(&inv.args);
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--series" && w[1] == "rabbitmq-4.2")
            );
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--series" && w[1] == "rabbitmq-4.1")
            );
            MockResponse::json_ok(batch_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .check()
        .batch()
        .series_scope([series("rabbitmq-4.2"), series("rabbitmq-4.1")])
        .commits([commit_sha(&"a".repeat(40))])
        .run()
        .expect("multi-series batch parses");
}

// `.commits` collapses duplicates and keeps first-occurrence order, so
// the row-per-commit contract holds for the stdin path.
#[test]
fn batch_builder_dedups_commits_preserving_order() {
    let a = "a".repeat(40);
    let b = "b".repeat(40);
    let c = "c".repeat(40);
    let expected = format!("{a}\n{b}\n{c}\n");
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckBatch), move |inv| {
            match &inv.stdin {
                StdinPayload::Bytes(bytes) => {
                    let framed = String::from_utf8(bytes.to_vec()).expect("utf8 commit list");
                    assert_eq!(framed, expected);
                }
                other => panic!("expected framed stdin, got {other:?}"),
            }
            MockResponse::json_ok(batch_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits([
            commit_sha(&a),
            commit_sha(&b),
            commit_sha(&a),
            commit_sha(&c),
        ])
        .run()
        .expect("deduped batch parses");
}

#[test]
fn batch_builder_commits_file_path_sends_no_stdin() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckBatch), |inv| {
            let args = args_of(&inv.args);
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--commits-file-path" && w[1] == "/tmp/commits.txt")
            );
            assert!(matches!(inv.stdin, StdinPayload::None));
            MockResponse::json_ok(batch_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits_file_path("/tmp/commits.txt")
        .run()
        .expect("file-path batch parses");
}

// `--target-ref` requires a target dir, so a ref set without one is
// dropped rather than sent alone.
#[test]
fn batch_builder_drops_target_ref_without_target_dir() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckBatch), |inv| {
            let args = args_of(&inv.args);
            assert!(!args.iter().any(|a| a == "--target-ref"));
            assert!(!args.iter().any(|a| a == "--target-repo-dir-path"));
            MockResponse::json_ok(batch_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .target_ref("v4.2.x")
        .commits([commit_sha(&"a".repeat(40))])
        .run()
        .expect("batch parses");
}

// The escape hatch sends bytes verbatim: no dedup, no reframing.
#[test]
fn batch_builder_commits_bytes_are_sent_verbatim() {
    let raw = b"deadbeef\ndeadbeef\n".to_vec();
    let expected = raw.clone();
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckBatch), move |inv| {
            match &inv.stdin {
                StdinPayload::Bytes(b) => assert_eq!(*b, expected.as_slice()),
                other => panic!("expected framed stdin, got {other:?}"),
            }
            MockResponse::json_ok(batch_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits_bytes(raw)
        .run()
        .expect("verbatim batch parses");
}

#[test]
fn batch_run_with_diagnostics_carries_argv() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckBatch),
            MockResponse::json_ok(batch_payload_fixture()),
        )
        .build();
    let driver = repo_driver(backend);
    let (_, executed) = driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits([commit_sha(&"a".repeat(40))])
        .run_with_diagnostics()
        .expect("batch parses");
    assert!(executed.argv.iter().any(|a| a == "check"));
    assert!(executed.argv.iter().any(|a| a == "batch"));
}

// The argv is an exact rerun and pins --formatter json onto the dispatch_typed
// path: program, verb, global flags, formatter, then builder args.
#[test]
fn batch_argv_orders_program_verb_globals_formatter_then_args() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckBatch),
            MockResponse::json_ok(batch_payload_fixture()),
        )
        .build();
    let driver = repo_driver(backend);
    let (_, executed) = driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits([commit_sha(&"a".repeat(40))])
        .run_with_diagnostics()
        .expect("batch parses");
    let argv: Vec<String> = executed
        .argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        vec![
            "backhopper",
            "check",
            "batch",
            "--repo-dir-path",
            "/repo",
            "--non-interactive",
            "--formatter",
            "json",
            "--series",
            "rabbitmq-4.2",
            "--commits-file-path",
            "-",
        ]
    );
}

// The schema ceiling is checked before the typed parse, so a too-new envelope
// reads as a clear mismatch rather than an unparseable-output error.
#[test]
fn batch_too_new_schema_is_a_mismatch_not_unparseable() {
    let envelope = json!({
        "schema_version": 999,
        "command": "check batch",
        "data": batch_payload_fixture(),
        "exit_code": 0
    });
    let bytes = serde_json::to_vec(&envelope).expect("envelope serialises");
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckBatch),
            MockResponse::Raw(RawOutcome::ok(bytes)),
        )
        .build();
    let driver = repo_driver(backend);
    let err = driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits([commit_sha(&"a".repeat(40))])
        .run()
        .expect_err("too-new schema");
    assert_eq!(err.kind(), ErrorKind::SchemaVersionMismatch);
}

// A batch row exposes the same accessors as a single check through the
// inherent evaluation() method.
#[test]
fn batch_row_evaluation_exposes_verdict_and_findings() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckBatch),
            MockResponse::json_ok(batch_payload_with_finding()),
        )
        .build();
    let driver = repo_driver(backend);
    let payload = driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits([commit_sha(&"a".repeat(40))])
        .run()
        .expect("batch parses");
    let eval = payload.results[0].evaluation();
    assert_eq!(eval.worst_verdict(), AggregateVerdict::RequiresAdaptation);
    let findings: Vec<_> = eval.test_module_symbol_missing().collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].missing_module.as_str(), "amqp_utils");
    assert_eq!(eval.missing_test_modules().len(), 1);
}

// A repo-operating verb with no repo dir is refused before any spawn:
// the mock would panic on invoke.
#[test]
fn batch_without_repo_dir_is_refused_before_spawn() {
    let backend = MockBackend::builder()
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .check()
        .batch()
        .series(series("rabbitmq-4.2"))
        .commits([commit_sha(&"a".repeat(40))])
        .run()
        .expect_err("missing repo dir");
    assert_eq!(err.kind(), ErrorKind::MissingRepoDir);
}
