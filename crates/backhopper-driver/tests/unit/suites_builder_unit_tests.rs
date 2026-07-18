// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::PathBuf;

use backhopper_driver::mock::{Fallthrough, MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::ApplicationName;
use backhopper_driver::{Backhopper, ErrorKind, StdinPayload, Verb};
use serde_json::{Value, json};

// Suites plan is repo-operating, so its driver needs a repo dir.
fn repo_driver(backend: MockBackend) -> Backhopper<MockBackend> {
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some(PathBuf::from("/repo"));
    driver
}

fn app(name: &str) -> ApplicationName {
    name.parse().expect("valid application name fixture")
}

fn suite_plan_fixture() -> Value {
    json!({ "entries": [] })
}

fn args_of(inv_args: &[Cow<'_, OsStr>]) -> Vec<String> {
    inv_args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn suites_plan_assembles_library_flags_and_stdin_paths() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::SuitesPlan), |inv| {
            let args = args_of(&inv.args);
            assert!(args.iter().any(|a| a == "--auto-library-apps"));
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--library-app" && w[1] == "rabbit")
            );
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--modified-paths-file-path" && w[1] == "-")
            );
            // the repo comes from the driver-global channel
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--repo-dir-path" && w[1] == "/repo")
            );
            match &inv.stdin {
                StdinPayload::Bytes(b) => {
                    let framed = String::from_utf8(b.to_vec()).expect("utf8 path list");
                    assert_eq!(framed, "deps/rabbit/src/a.erl\n");
                }
                other => panic!("expected framed stdin, got {other:?}"),
            }
            MockResponse::json_ok(suite_plan_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    let plan = driver
        .suites()
        .plan()
        .auto_library_apps(true)
        .library_app(app("rabbit"))
        .modified_paths(["deps/rabbit/src/a.erl"])
        .run()
        .expect("suite plan parses");
    assert!(plan.entries.is_empty());
}

#[test]
fn suites_plan_file_path_sends_no_stdin() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::SuitesPlan), |inv| {
            let args = args_of(&inv.args);
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--modified-paths-file-path" && w[1] == "/tmp/paths.txt")
            );
            assert!(matches!(inv.stdin, StdinPayload::None));
            MockResponse::json_ok(suite_plan_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .suites()
        .plan()
        .modified_paths_file_path("/tmp/paths.txt")
        .run()
        .expect("file-path plan parses");
}

#[test]
fn suites_plan_emits_extra_rules_file_path() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::SuitesPlan), |inv| {
            let args = args_of(&inv.args);
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--extra-rules-file-path" && w[1] == "/tmp/rules.toml")
            );
            MockResponse::json_ok(suite_plan_fixture())
        })
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = repo_driver(backend);
    driver
        .suites()
        .plan()
        .extra_rules_file_path("/tmp/rules.toml")
        .modified_paths(["src/a.erl"])
        .run()
        .expect("plan parses");
}

#[test]
fn suites_plan_run_with_diagnostics_carries_argv() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::SuitesPlan),
            MockResponse::json_ok(suite_plan_fixture()),
        )
        .build();
    let driver = repo_driver(backend);
    let (_, executed) = driver
        .suites()
        .plan()
        .modified_paths(["src/a.erl"])
        .run_with_diagnostics()
        .expect("plan parses");
    assert!(executed.argv.iter().any(|a| a == "suites"));
    assert!(executed.argv.iter().any(|a| a == "plan"));
}

// suites plan is repo-operating: a driver with no repo dir is refused before spawn
#[test]
fn suites_plan_without_repo_dir_is_refused_before_spawn() {
    let backend = MockBackend::builder()
        .fallthrough(Fallthrough::Panic)
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .suites()
        .plan()
        .modified_paths(["src/a.erl"])
        .run()
        .expect_err("missing repo dir");
    assert_eq!(err.kind(), ErrorKind::MissingRepoDir);
}
