// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::borrow::Cow;
use std::ffi::OsStr;

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::{ProjectName, SeriesName, TagName};
use backhopper_driver::{Backhopper, Verb};
use serde_json::{Value, json};

fn project(name: &str) -> ProjectName {
    name.parse().expect("valid project name fixture")
}

fn tag(name: &str) -> TagName {
    name.parse().expect("valid tag name fixture")
}

fn series(name: &str) -> SeriesName {
    name.parse().expect("valid series name fixture")
}

fn diff_payload_fixture() -> Value {
    json!({
        "project": "ra",
        "from": "v2.13.0",
        "to": "v2.16.0",
        "modules_added": [],
        "modules_removed": [],
        "exports_added": [],
        "exports_removed": [],
        "types_added": [],
        "types_removed": [],
        "callbacks_added": [],
        "callbacks_removed": [],
        "headers_added": [],
        "headers_removed": [],
        "records_added": [],
        "records_removed": []
    })
}

fn cross_series_fixture() -> Value {
    json!({ "from_series": "rabbitmq-4.1", "to_series": "rabbitmq-4.2", "projects": [] })
}

fn args_of(inv_args: &[Cow<'_, OsStr>]) -> Vec<String> {
    inv_args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn project_diff_assembles_project_from_to() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::SnapshotsProjectDiff), |inv| {
            let args = args_of(&inv.args);
            assert!(args.windows(2).any(|w| w[0] == "--project" && w[1] == "ra"));
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--from" && w[1] == "v2.13.0")
            );
            assert!(args.windows(2).any(|w| w[0] == "--to" && w[1] == "v2.16.0"));
            MockResponse::json_ok(diff_payload_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = Backhopper::with_backend(backend);
    let diff = driver
        .snapshots()
        .project_diff(project("ra"), tag("v2.13.0"), tag("v2.16.0"))
        .run()
        .expect("diff parses");
    assert_eq!(diff.project, "ra");
}

#[test]
fn series_diff_assembles_from_and_to_series() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::SnapshotsSeriesDiff), |inv| {
            let args = args_of(&inv.args);
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--from-series" && w[1] == "rabbitmq-4.1")
            );
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--to-series" && w[1] == "rabbitmq-4.2")
            );
            MockResponse::json_ok(cross_series_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = Backhopper::with_backend(backend);
    let diff = driver
        .snapshots()
        .series_diff(series("rabbitmq-4.1"), series("rabbitmq-4.2"))
        .run()
        .expect("cross-series diff parses");
    assert_eq!(diff.from_series, "rabbitmq-4.1");
    assert!(diff.projects.is_empty());
}

// The argv is an exact rerun with no repo dir set: program, verb, the default
// global flags, --formatter json, then the builder args.
#[test]
fn series_diff_argv_orders_program_verb_formatter_then_args() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::SnapshotsSeriesDiff),
            MockResponse::json_ok(cross_series_fixture()),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let (_, executed) = driver
        .snapshots()
        .series_diff(series("rabbitmq-4.1"), series("rabbitmq-4.2"))
        .run_with_diagnostics()
        .expect("cross-series diff parses");
    let argv: Vec<String> = executed
        .argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        vec![
            "backhopper",
            "snapshots",
            "series_diff",
            "--non-interactive",
            "--formatter",
            "json",
            "--from-series",
            "rabbitmq-4.1",
            "--to-series",
            "rabbitmq-4.2",
        ]
    );
}

// Snapshots diff is repo-free: it dispatches with no repo dir set.
#[test]
fn project_diff_run_with_diagnostics_carries_argv() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::SnapshotsProjectDiff),
            MockResponse::json_ok(diff_payload_fixture()),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let (_, executed) = driver
        .snapshots()
        .project_diff(project("ra"), tag("v2.13.0"), tag("v2.16.0"))
        .run_with_diagnostics()
        .expect("diff parses");
    assert!(executed.argv.iter().any(|a| a == "snapshots"));
    assert!(executed.argv.iter().any(|a| a == "project_diff"));
}
