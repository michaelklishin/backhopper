// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_driver::{Backhopper, ExplainFormat, Verb, VerbId};
use serde_json::{Value, json};

fn series(name: &str) -> SeriesName {
    name.parse().expect("valid series name fixture")
}

fn commit_sha(hex: &str) -> CommitSha {
    hex.parse().expect("valid commit SHA fixture")
}

fn empty_evaluation_fixture() -> Value {
    json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": []
        },
        "results": {
            "results": [],
            "summary": { "compatible": 0, "requires_adaptation": 0, "incompatible": 0 }
        },
        "diagnostics": {}
    })
}

#[test]
fn patch_builder_injects_target_input_and_options() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckPatch), move |inv| {
            let args: Vec<String> = inv
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--series" && w[1] == "rabbitmq-4.2")
            );
            assert!(args.iter().any(|a| a == "--explain"));
            assert!(args.iter().any(|a| a == "--suggest-prereqs"));
            MockResponse::json_ok(empty_evaluation_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = Backhopper::with_backend(backend);
    let evaluation = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"".to_vec())
        .explain(ExplainFormat::Markdown)
        .suggest_prereqs(true)
        .run()
        .unwrap();
    assert_eq!(evaluation.results.summary.compatible, 0);
}

#[test]
fn run_with_diagnostics_returns_executed_invocation() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::CheckPatch),
            MockResponse::json_ok(empty_evaluation_fixture()),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let (evaluation, executed) = driver
        .check()
        .patch()
        .series(series("rabbitmq-4.2"))
        .patch_bytes(b"abc".to_vec())
        .run_with_diagnostics()
        .unwrap();
    assert_eq!(evaluation.results.summary.compatible, 0);
    assert_eq!(executed.exit_code, 0);
    assert_eq!(executed.stdin_bytes, Some(3));
    assert!(!executed.argv.is_empty());
    assert!(matches!(executed.verb, VerbId::Known(Verb::CheckPatch)));
}

#[test]
fn commit_builder_routes_to_check_commit_verb() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckCommit), |inv| {
            let last = inv
                .args
                .last()
                .expect("commit arg")
                .to_string_lossy()
                .to_string();
            assert_eq!(last, "0123456789abcdef0123456789abcdef01234567");
            MockResponse::json_ok(empty_evaluation_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some("/repo".into());
    let _ = driver
        .check()
        .commit()
        .series(series("rabbitmq-4.2"))
        .commit(commit_sha("0123456789abcdef0123456789abcdef01234567"))
        .run()
        .unwrap();
}

#[test]
fn range_builder_appends_positional_range_argument() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckRange), |inv| {
            let last = inv
                .args
                .last()
                .expect("range arg")
                .to_string_lossy()
                .to_string();
            assert_eq!(last, "BASE..HEAD");
            MockResponse::json_ok(empty_evaluation_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some("/repo".into());
    let _ = driver
        .check()
        .range()
        .series(series("rabbitmq-4.2"))
        .range("BASE..HEAD")
        .run()
        .unwrap();
}

#[test]
fn merge_builder_appends_positional_merge_sha() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckMerge), |inv| {
            let last = inv.args.last().unwrap().to_string_lossy().to_string();
            assert_eq!(last, "fedcba9876543210fedcba9876543210fedcba98");
            MockResponse::json_ok(empty_evaluation_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some("/repo".into());
    let _ = driver
        .check()
        .merge()
        .series(series("rabbitmq-4.2"))
        .merge_commit(commit_sha("fedcba9876543210fedcba9876543210fedcba98"))
        .run()
        .unwrap();
}

#[test]
fn pin_target_emits_project_and_tag_flags() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::CheckPatch), |inv| {
            let args: Vec<String> = inv
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(args.windows(2).any(|w| w[0] == "--project" && w[1] == "ra"));
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--tag" && w[1] == "v2.16.7")
            );
            MockResponse::json_ok(empty_evaluation_fixture())
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let project: ProjectName = "ra".parse().unwrap();
    let tag: TagName = "v2.16.7".parse().unwrap();
    let _ = driver
        .check()
        .patch()
        .pin(project, tag)
        .patch_bytes(b"".to_vec())
        .run()
        .unwrap();
}
