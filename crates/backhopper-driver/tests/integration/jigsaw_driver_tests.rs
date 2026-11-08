// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `jigsaw` verbs through a real `SubprocessBackend` against a shell stub
//! that records its argv and answers with a canned envelope. The real
//! binary is driven from `backhopper-cli`'s `jigsaw_check_tests`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use backhopper_driver::types::{
    AppRole, ApplicationName, HostName, HostSeries, JigsawCell, NotChecked, TagSourceKind,
    WithheldReason,
};
use backhopper_driver::{
    Backhopper, CURRENT_ENVELOPE_VERSION, ErrorKind, JigsawAppRowPayload, JigsawCheckPayload,
    JigsawTalliesPayload,
};
use serde_json::json;
use tempfile::TempDir;

struct Stub {
    dir: TempDir,
}

impl Stub {
    fn answering(command: &str, data: &serde_json::Value, exit_code: i32) -> Self {
        let dir = TempDir::new().unwrap();
        let envelope = json!({
            "schema_version": CURRENT_ENVELOPE_VERSION,
            "command": command,
            "data": data,
            "exit_code": exit_code,
        });
        fs::write(dir.path().join("envelope.json"), envelope.to_string()).unwrap();
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{dir}/argv.txt\"\ncat \"{dir}/envelope.json\"\nexit {exit_code}\n",
            dir = dir.path().display()
        );
        let path = dir.path().join("backhopper");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }

    fn driver(&self) -> Backhopper {
        Backhopper::with_binary_path(self.dir.path().join("backhopper"))
    }

    fn argv(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("argv.txt"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn report(clean: bool) -> JigsawCheckPayload {
    JigsawCheckPayload {
        host: "rabbitmq".to_owned(),
        series: "4.2".to_owned(),
        tag_source: TagSourceKind::Store,
        build_release: "v4.2.0".to_owned(),
        host_releases: vec!["v4.2.0".to_owned()],
        otp_tags: vec!["OTP-27.3".to_owned()],
        apps: vec![JigsawAppRowPayload {
            app: ApplicationName::new("rabbitmq_shovel_extra").unwrap(),
            role: AppRole::Plugin,
            cells: vec![if clean {
                JigsawCell::Clean
            } else {
                JigsawCell::Broken
            }],
        }],
        findings: Vec::new(),
        warnings: Vec::new(),
        host_provided: Vec::new(),
        unchecked: Vec::new(),
        withheld: Vec::new(),
        tallies: JigsawTalliesPayload {
            checked: 7,
            not_checked: NotChecked::ALL.iter().map(|&r| (r, 0)).collect(),
            withheld: WithheldReason::ALL.iter().map(|&r| (r, 0)).collect(),
        },
        limits: Vec::new(),
        store_digest: "0123".to_owned(),
        clean,
    }
}

#[test]
fn check_parses_a_report_that_exited_with_the_partial_success_code() {
    let stub = Stub::answering("jigsaw check", &json!(report(false)), 3);
    let parsed = stub
        .driver()
        .jigsaw()
        .check()
        .plan_file(PathBuf::from("plan.json"))
        .plugin_dir("plugins")
        .plugin_dir("vendor")
        .run()
        .unwrap();
    assert_eq!(parsed, report(false));
    let argv = stub.argv();
    assert_eq!(&argv[..2], ["jigsaw", "check"]);
    assert!(
        argv.windows(2)
            .any(|w| w == ["--plugin-dir-path", "vendor"])
    );
}

#[test]
fn check_with_an_unstocked_plan_is_a_tool_error() {
    let stub = Stub::answering("jigsaw check", &json!(null), 65);
    let err = stub
        .driver()
        .jigsaw()
        .check()
        .plan_file("plan.json")
        .plugin_dir("plugins")
        .run()
        .expect_err("65 is a data error");
    assert_eq!(err.kind(), ErrorKind::ToolError);
}

#[test]
fn plan_names_its_verb_and_arguments_on_the_command_line() {
    let stub = Stub::answering("jigsaw plan", &json!(null), 0);
    let _ = stub
        .driver()
        .jigsaw()
        .plan()
        .host(HostName::new("rabbitmq").unwrap())
        .series("4.2".parse::<HostSeries>().unwrap())
        .run();
    let argv = stub.argv();
    assert_eq!(&argv[..2], ["jigsaw", "plan"]);
    assert!(argv.windows(2).any(|w| w == ["--host", "rabbitmq"]));
    assert!(argv.windows(2).any(|w| w == ["--series", "4.2"]));
    assert!(argv.windows(2).any(|w| w == ["--formatter", "json"]));
}
