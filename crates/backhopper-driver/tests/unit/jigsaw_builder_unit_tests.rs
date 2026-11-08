// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "mock")]

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;

use backhopper_driver::mock::{MockBackend, MockMatcher, MockResponse};
use backhopper_driver::types::{
    AppRole, ApplicationName, ErlangMajor, HostName, HostSeries, JigsawCell, NotChecked,
    SnapshotGap, TagName, TagSourceKind, WithheldReason,
};
use backhopper_driver::{
    Backhopper, ErrorKind, JigsawAppRowPayload, JigsawCheckPayload, JigsawPlanPayload,
    JigsawTalliesPayload, MissingSnapshotPayload, PlanArguments, PlanOtpLinePayload,
    PlanReleasePayload, Verb,
};

fn args_of(args: &[Cow<'_, OsStr>]) -> Vec<String> {
    args.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2).any(|w| w[0] == flag && w[1] == value)
}

fn host() -> HostName {
    "rabbitmq".parse().unwrap()
}

fn series() -> HostSeries {
    "4.2".parse().unwrap()
}

fn plan_payload(missing: bool) -> JigsawPlanPayload {
    let missing_snapshots = if missing {
        vec![MissingSnapshotPayload {
            project: "ra".to_owned(),
            tag: "v2.17.3".to_owned(),
            reason: SnapshotGap::Missing,
            detail: None,
        }]
    } else {
        Vec::new()
    };
    JigsawPlanPayload {
        host: "rabbitmq".to_owned(),
        host_project: "rabbitmq-server".to_owned(),
        otp_project: "otp".to_owned(),
        series: "4.2".to_owned(),
        tag_source: "repository".to_owned(),
        extractor_version: "9".to_owned(),
        host_facts_digest: "abcd".to_owned(),
        releases: vec![PlanReleasePayload {
            tag: "v4.2.0".to_owned(),
            commit: "0".repeat(40),
            pins: BTreeMap::new(),
            erlang_support: "[27.0,29.0)".to_owned(),
        }],
        otp_lines: vec![PlanOtpLinePayload {
            line: "27.3".to_owned(),
            tags: vec!["OTP-27.3".to_owned()],
        }],
        untracked_dependencies: Vec::new(),
        excluded_otp_apps: Vec::new(),
        required_snapshots: Vec::new(),
        missing_snapshots,
        cache_key: "jigsaw-rabbitmq-4.2-ab-cd-1234".to_owned(),
        restore_key: "jigsaw-rabbitmq-4.2-ab-cd-".to_owned(),
    }
}

fn check_payload(clean: bool) -> JigsawCheckPayload {
    JigsawCheckPayload {
        host: "rabbitmq".to_owned(),
        series: "4.2".to_owned(),
        tag_source: TagSourceKind::Repository,
        build_release: "v4.2.0".to_owned(),
        host_releases: vec!["v4.2.0".to_owned()],
        otp_tags: vec!["OTP-27.3".to_owned()],
        apps: vec![JigsawAppRowPayload {
            app: ApplicationName::new("rabbitmq_shovel_extra").unwrap(),
            role: AppRole::Plugin,
            cells: vec![JigsawCell::Clean],
        }],
        findings: Vec::new(),
        warnings: Vec::new(),
        host_provided: Vec::new(),
        unchecked: Vec::new(),
        withheld: Vec::new(),
        tallies: JigsawTalliesPayload {
            checked: 3,
            not_checked: NotChecked::ALL.iter().map(|&r| (r, 0)).collect(),
            withheld: WithheldReason::ALL.iter().map(|&r| (r, 0)).collect(),
        },
        limits: vec!["OTP header records and macros are not compared".to_owned()],
        store_digest: "0123".to_owned(),
        clean,
    }
}

#[test]
fn plan_assembles_its_arguments_and_returns_a_plan_with_missing_snapshots() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::JigsawPlan), |inv| {
            let args = args_of(&inv.args);
            assert!(has_pair(&args, "--host", "rabbitmq"));
            assert!(has_pair(&args, "--series", "4.2"));
            assert!(has_pair(&args, "--up-to", "v4.2.3"));
            assert!(has_pair(&args, "--erlang-major", "27"));
            assert!(has_pair(&args, "--erlang-major", "28"));
            assert!(has_pair(&args, "--plan-file-path", "plan.json"));
            MockResponse::jigsaw_plan(&plan_payload(true))
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let (plan, executed) = driver
        .jigsaw()
        .plan()
        .series(series())
        .host(host())
        .up_to(TagName::new("v4.2.3").unwrap())
        .erlang_major(ErlangMajor::try_from(27).unwrap())
        .erlang_major(ErlangMajor::try_from(28).unwrap())
        .plan_file_path("plan.json")
        .run_with_diagnostics()
        .expect("a plan with missing snapshots is a result");
    assert_eq!(plan.missing_snapshots.len(), 1);
    assert_eq!(executed.exit_code, 3);
    assert_eq!(executed.verb.display_name(), "jigsaw plan");
}

#[test]
fn plan_passes_the_global_repo_dir_and_check_does_not() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::JigsawPlan), |inv| {
            assert!(has_pair(&args_of(&inv.args), "--repo-dir-path", "/host"));
            MockResponse::jigsaw_plan(&plan_payload(false))
        })
        .respond_with(MockMatcher::Verb(Verb::JigsawCheck), |inv| {
            assert!(!args_of(&inv.args).contains(&"--repo-dir-path".to_owned()));
            MockResponse::jigsaw_check(&check_payload(true))
        })
        .build();
    let mut driver = Backhopper::with_backend(backend);
    driver.options_mut().repo_dir_path = Some(PathBuf::from("/host"));
    driver
        .jigsaw()
        .plan()
        .host(host())
        .series(series())
        .run()
        .unwrap();
    driver
        .jigsaw()
        .check()
        .plan_file("plan.json")
        .plugin_dir("plugins")
        .run()
        .unwrap();
}

#[test]
fn check_with_a_plan_file_passes_every_plugin_directory_in_order() {
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::JigsawCheck), |inv| {
            let args = args_of(&inv.args);
            assert!(has_pair(&args, "--plan-file-path", "plan.json"));
            let dirs: Vec<&str> = args
                .windows(2)
                .filter(|w| w[0] == "--plugin-dir-path")
                .map(|w| w[1].as_str())
                .collect();
            assert_eq!(dirs, ["plugins", "vendor", "extra"]);
            assert!(!args.contains(&"--host".to_owned()));
            MockResponse::jigsaw_check(&check_payload(false))
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let report = driver
        .jigsaw()
        .check()
        .plugin_dir("plugins")
        .plugin_dir("vendor")
        .plan_file("plan.json")
        .plugin_dir("extra")
        .run()
        .expect("a report with findings is a result");
    assert!(!report.clean);
    assert_eq!(report.apps[0].cells, [JigsawCell::Clean]);
}

#[test]
fn check_with_plan_arguments_drafts_the_plan_from_the_store() {
    let arguments = PlanArguments::new(host(), series())
        .up_to(TagName::new("v4.2.3").unwrap())
        .erlang_major(ErlangMajor::try_from(27).unwrap());
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::JigsawCheck), |inv| {
            let args = args_of(&inv.args);
            assert!(has_pair(&args, "--host", "rabbitmq"));
            assert!(has_pair(&args, "--series", "4.2"));
            assert!(has_pair(&args, "--up-to", "v4.2.3"));
            assert!(has_pair(&args, "--erlang-major", "27"));
            assert!(!args.contains(&"--plan-file-path".to_owned()));
            MockResponse::jigsaw_check(&check_payload(true))
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    let report = driver
        .jigsaw()
        .check()
        .plan_arguments(arguments)
        .plugin_dir("plugins")
        .run()
        .unwrap();
    assert!(report.clean);
}

#[test]
fn a_plan_builder_hands_its_arguments_to_check() {
    let driver = Backhopper::with_backend(MockBackend::builder().build());
    let arguments = driver
        .jigsaw()
        .plan()
        .host(host())
        .series(series())
        .plan_file_path("ignored.json")
        .into_arguments();
    let backend = MockBackend::builder()
        .respond_with(MockMatcher::Verb(Verb::JigsawCheck), |inv| {
            let args = args_of(&inv.args);
            assert!(has_pair(&args, "--host", "rabbitmq"));
            assert!(!args.iter().any(|a| a == "ignored.json"));
            MockResponse::jigsaw_check(&check_payload(true))
        })
        .build();
    let driver = Backhopper::with_backend(backend);
    driver
        .jigsaw()
        .check()
        .plan_arguments(arguments)
        .plugin_dir("plugins")
        .run()
        .unwrap();
}

#[test]
fn an_unstocked_plan_is_a_tool_error_with_the_data_error_code() {
    let backend = MockBackend::builder()
        .respond_to(
            MockMatcher::Verb(Verb::JigsawCheck),
            MockResponse::tool_error("plan is not stocked", 65),
        )
        .build();
    let driver = Backhopper::with_backend(backend);
    let err = driver
        .jigsaw()
        .check()
        .plan_file("plan.json")
        .plugin_dir("plugins")
        .run()
        .expect_err("65 is not success-shaped");
    assert_eq!(err.kind(), ErrorKind::ToolError);
}

#[test]
fn canned_envelopes_carry_the_current_schema_version_and_exit_codes() {
    let clean = MockResponse::jigsaw_check(&check_payload(true));
    let broken = MockResponse::jigsaw_check(&check_payload(false));
    let stocked = MockResponse::jigsaw_plan(&plan_payload(false));
    for (response, code) in [(clean, 0), (broken, 3), (stocked, 0)] {
        match response {
            MockResponse::Json {
                exit_code,
                schema_version,
                ..
            } => {
                assert_eq!(exit_code, code);
                assert_eq!(
                    schema_version.get(),
                    backhopper_driver::CURRENT_ENVELOPE_VERSION
                );
            }
            other => panic!("expected a JSON response, got {other:?}"),
        }
    }
}
