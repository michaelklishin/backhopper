// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use backhopper_driver::mock::{MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::{GitRef, SeriesName, SiblingDriftAction, SinceDerivation};
use backhopper_driver::{Backhopper, SchemaVersion, Verb, VerbId};
use serde_json::{Value, json};

fn series(name: &str) -> SeriesName {
    name.parse().expect("valid series name fixture")
}

fn git_ref(name: &str) -> GitRef {
    name.parse().expect("valid git ref fixture")
}

fn doctor_report_fixture() -> Value {
    json!({
        "series": "rabbitmq-4.0",
        "target_branch": "v4.0.x",
        "source_branches": ["main"],
        "since": {
            "kind": "last_release_tag",
            "tag": "v4.0.9",
            "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        "vocabulary_source": "family_default",
        "suppressed_count": 14,
        "walked_count": 487,
        "candidates": [
            {
                "sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "subject": "Don't find crashes in logs in cluster_SUITE",
                "committed_at": "2025-10-04T12:00:00Z",
                "age_days": 248,
                "touched_paths": ["deps/rabbit/test/cluster_SUITE.erl"],
                "parent_count": 1,
                "confidence": 0.84,
                "action": "sweep"
            }
        ]
    })
}

#[test]
fn doctor_report_round_trips_through_the_typed_builder() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::SiblingsDoctor),
            MockResponse::json_ok(doctor_report_fixture()),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let (report, executed) = driver
        .siblings()
        .doctor(&series("rabbitmq-4.0"))
        .run()
        .expect("report parses");

    assert_eq!(report.series.as_str(), "rabbitmq-4.0");
    assert_eq!(report.suppressed_count, 14);
    assert_eq!(report.walked_count, 487);
    assert!(matches!(
        report.since,
        SinceDerivation::LastReleaseTag { .. }
    ));
    assert_eq!(report.candidates.len(), 1);
    let candidate = &report.candidates[0];
    assert_eq!(candidate.action(), SiblingDriftAction::Sweep);
    assert!(candidate.confidence().value() > 0.8);
    assert!(matches!(executed.verb, VerbId::Known(Verb::SiblingsDoctor)));
}

#[test]
fn builder_assembles_every_optional_flag() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::SiblingsDoctor),
            MockResponse::json_ok(doctor_report_fixture()),
        )
        .build();
    let recorded = {
        let driver = Backhopper::with_backend(backend);
        let _ = driver
            .siblings()
            .doctor(&series("rabbitmq-4.0"))
            .source_branch(git_ref("main"))
            .source_branch(git_ref("v4.1.x"))
            .target_branch(git_ref("v4.0.x"))
            .since("v4.0.9")
            .top(10)
            .vocabulary_file_path("/tmp/vocab.txt")
            .explain()
            .no_cache()
            .run()
            .unwrap();
        driver.backend().invocations()
    };
    assert_eq!(recorded.len(), 1);
    let args: Vec<String> = recorded[0]
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    for expected in [
        "--series",
        "rabbitmq-4.0",
        "--source-branch",
        "v4.1.x",
        "--target-branch",
        "v4.0.x",
        "--since",
        "v4.0.9",
        "--top",
        "10",
        "--vocabulary-file-path",
        "--explain",
        "--no-cache",
    ] {
        assert!(
            args.iter().any(|a| a == expected),
            "missing {expected} in {args:?}"
        );
    }
    // the driver always forces machine-readable output
    assert!(args.iter().any(|a| a == "--formatter"));
}

#[test]
fn version_payload_verbs_probe_surfaces_siblings_doctor() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::Json {
                payload: json!({
                    "name": "backhopper",
                    "version": "0.13.0",
                    "verbs": ["check batch", "siblings doctor", "version"]
                }),
                exit_code: 0,
                schema_version: SchemaVersion::new(8),
                warnings: Vec::new(),
            },
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let info = driver.version().expect("version parses");
    assert_eq!(info.version, "0.13.0");
    assert_eq!(info.schema_version, SchemaVersion::new(8));
    assert!(info.has_verb("siblings doctor"));
    assert!(!info.has_verb("cache list"));
}

#[test]
fn version_payload_without_verbs_yields_an_empty_capability_list() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::Version),
            MockResponse::Json {
                payload: json!({ "name": "backhopper", "version": "0.12.0" }),
                exit_code: 0,
                schema_version: SchemaVersion::new(7),
                warnings: Vec::new(),
            },
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let info = driver.version().expect("version parses");
    assert!(info.verbs.is_empty());
    assert!(!info.has_verb("siblings doctor"));
}
