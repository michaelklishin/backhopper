// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use backhopper_test_support::{GitRepoFixture, toml_path};

use crate::helpers::cli::run;
use crate::helpers::jigsaw::{PARTIAL_SUCCESS, Workspace};

const CONFIG_ERROR: i32 = 78;

fn host_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "lib/queue_app/src/queue_app.erl",
        "-module(queue_app).\n-export([run/0]).\nrun() -> ok.\n",
    );
    repo.write_file(
        "lib/queue_app/Makefile",
        "PROJECT = queue_app\nDEPS = widget\n",
    );
    repo.write_file("plugins.mk", "PLUGINS = queue_app\n");
    for (release, widget) in [
        ("v1.0.0", "1.0.0"),
        ("v1.0.1", "1.0.0"),
        ("v1.0.2", "1.1.0"),
    ] {
        repo.write_file(
            "deps.mk",
            &format!("dep_widget = hex {widget}\ndep_jose = hex 1.11.10\n"),
        );
        repo.write_file("RELEASE", release);
        repo.commit(release);
        repo.tag(release);
    }
    repo
}

fn widget_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "src/widget.erl",
        "-module(widget).\n-export([spin/0]).\nspin() -> ok.\n",
    );
    repo.commit("widget 1.0.0");
    repo.tag("v1.0.0");
    repo.write_file(
        "src/widget.erl",
        "-module(widget).\n-export([spin/0, stop/0]).\nspin() -> ok.\nstop() -> ok.\n",
    );
    repo.commit("widget 1.1.0");
    repo.tag("v1.1.0");
    repo
}

fn otp_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "lib/kernel/src/kernel.erl",
        "-module(kernel).\n-export([stop/0]).\nstop() -> ok.\n",
    );
    repo.commit("otp");
    for tag in ["OTP-27.3", "OTP-29.1"] {
        repo.tag(tag);
    }
    repo
}

const ERLANG_MK_PINS: &str = "[host.pins]\nsource = \"erlang_mk\"\nfile_path = \"deps.mk\"\n";

fn workspace() -> Workspace {
    workspace_with_pins(ERLANG_MK_PINS)
}

fn workspace_with_pins(pins: &str) -> Workspace {
    let (host, widget, otp) = (host_repo(), widget_repo(), otp_repo());
    let paths = (
        host.path().to_owned(),
        widget.path().to_owned(),
        otp.path().to_owned(),
    );
    Workspace::new(vec![host, widget, otp], |dir| {
        config_text(dir, &paths.0, &paths.1, &paths.2, pins)
    })
}

fn config_text(dir: &Path, host: &Path, widget: &Path, otp: &Path, pins: &str) -> String {
    format!(
        r#"
config_version = 1

[defaults]
snapshot_dir = "{snapshots}"

[[project]]
name = "queue-host"
git_url = "{host}"
layout = "multi_app"
app_roots = ["lib/*"]
manifest = "erlang_mk"

[[project]]
name = "widget"
git_url = "{widget}"

[[project]]
name = "otp"
git_url = "{otp}"
layout = "erlang_otp"

[[host]]
name = "queue"
project = "queue-host"
otp_project = "otp"
series_tag_pattern = "v{{series}}.*"
erlang_majors = [27]

{pins}
[host.release_roots]
makefile_variable = {{ file_path = "plugins.mk", variable = "PLUGINS" }}

[host.erlang_support]
default = "[27.0,29.0)"
"#,
        snapshots = toml_path(&dir.join("snapshots")),
        host = toml_path(host),
        widget = toml_path(widget),
        otp = toml_path(otp),
        pins = pins,
    )
}

fn plan_args<'a>(plan_file: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "jigsaw",
        "plan",
        "--host",
        "queue",
        "--series",
        "1.0",
        "--plan-file-path",
        plan_file,
    ];
    args.extend_from_slice(extra);
    args
}

#[test]
fn a_cold_store_plans_three_releases_and_exits_with_partial_success() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let (code, json) = ws.json(&plan_args(plan_file.to_str().unwrap(), &[]));
    assert_eq!(code, PARTIAL_SUCCESS, "{json}");
    let data = &json["data"];
    assert_eq!(data["releases"].as_array().unwrap().len(), 3);
    assert_eq!(data["tag_source"], "repository");
    let missing = data["missing_snapshots"].as_array().unwrap();
    assert_eq!(missing.len(), 6, "3 host, 2 widget, 1 OTP: {missing:?}");
    assert!(missing.iter().all(|m| m["reason"] == "missing"));
    let widget: Vec<&Value> = missing
        .iter()
        .filter(|m| m["project"] == "widget")
        .collect();
    assert_eq!(widget.len(), 2);
    assert_eq!(data["untracked_dependencies"][0], "jose");
    assert!(
        data["cache_key"]
            .as_str()
            .unwrap()
            .starts_with("jigsaw-queue-1.0-f5-e")
    );
    assert!(plan_file.exists());
}

#[test]
fn the_plan_file_is_the_json_envelope() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let (_, json) = ws.json(&plan_args(plan_file.to_str().unwrap(), &[]));
    let written: Value = serde_json::from_str(&fs::read_to_string(&plan_file).unwrap()).unwrap();
    assert_eq!(written, json);
    assert_eq!(written["command"], "jigsaw plan");
}

#[test]
fn generating_from_the_plan_stocks_the_store_and_a_second_plan_exits_zero() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    let (_, first) = ws.json(&plan_args(plan_path, &[]));

    let (code, generated) = ws.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
    assert_eq!(code, 0, "{generated}");
    assert_eq!(generated["data"]["generated"].as_array().unwrap().len(), 6);
    assert!(generated["data"]["failed"].as_array().unwrap().is_empty());

    let (code, second) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, 0, "{second}");
    assert!(
        second["data"]["missing_snapshots"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(second["data"]["cache_key"], first["data"]["cache_key"]);
}

#[test]
fn generating_from_a_stocked_plan_does_nothing() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    ws.json(&plan_args(plan_path, &[]));
    ws.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
    let (code, again) = ws.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
    assert_eq!(code, 0);
    assert!(again["data"]["generated"].as_array().unwrap().is_empty());
}

#[test]
fn a_dry_run_generates_nothing() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    ws.json(&plan_args(plan_path, &[]));
    let (code, dry) = ws.json(&[
        "snapshots",
        "generate",
        "--from-jigsaw-plan",
        plan_path,
        "--dry-run",
    ]);
    assert_eq!(code, 0);
    assert_eq!(dry["data"]["generated"].as_array().unwrap().len(), 6);
    let (code, _) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, PARTIAL_SUCCESS);
}

#[test]
fn up_to_limits_the_releases() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let (_, json) = ws.json(&plan_args(
        plan_file.to_str().unwrap(),
        &["--up-to", "v1.0.1"],
    ));
    assert_eq!(json["data"]["releases"].as_array().unwrap().len(), 2);
}

#[test]
fn the_text_output_lists_releases_otp_lines_missing_snapshots_and_the_cache_key() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let (code, text) = ws.text(&plan_args(plan_file.to_str().unwrap(), &[]));
    assert_eq!(code, PARTIAL_SUCCESS);
    assert!(
        text.contains("jigsaw plan: queue 1.0, built against v1.0.2"),
        "{text}"
    );
    assert!(text.contains("v1.0.1"), "{text}");
    assert!(text.contains("widget v1.0.0 -> v1.1.0"), "{text}");
    assert!(text.contains("27.3: OTP-27.3 (1 tag)"), "{text}");
    assert!(text.contains("missing widget v1.1.0"), "{text}");
    assert!(text.contains("cache key: jigsaw-queue-1.0-f5-e"), "{text}");
    assert!(
        text.contains("restore key: jigsaw-queue-1.0-f5-e"),
        "{text}"
    );
}

#[test]
fn an_unsupported_erlang_major_is_a_configuration_error() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let (code, _) = ws.json(&plan_args(
        plan_file.to_str().unwrap(),
        &["--erlang-major", "29"],
    ));
    assert_eq!(code, CONFIG_ERROR);
}

#[test]
fn an_empty_series_is_a_configuration_error() {
    let ws = workspace();
    let (code, _) = ws.json(&["jigsaw", "plan", "--host", "queue", "--series", "9.9"]);
    assert_eq!(code, CONFIG_ERROR);
}

#[test]
fn an_unknown_host_is_a_configuration_error() {
    let ws = workspace();
    let (code, _) = ws.json(&["jigsaw", "plan", "--host", "zeromq", "--series", "1.0"]);
    assert_eq!(code, CONFIG_ERROR);
}

#[test]
fn from_jigsaw_plan_conflicts_with_the_other_ways_to_pick_what_to_generate() {
    let ws = workspace();
    for conflicting in [
        ["--project", "widget"],
        ["--since", "v1.0.0"],
        ["--series", "queue"],
    ] {
        let mut args = vec!["snapshots", "generate", "--from-jigsaw-plan", "plan.json"];
        args.extend(conflicting);
        let (code, _) = ws.json(&args);
        assert_eq!(code, 2, "{conflicting:?}");
    }
}

#[test]
fn a_plan_file_that_is_not_a_plan_envelope_is_refused() {
    let ws = workspace();
    let not_a_plan = ws.path("other.json");
    fs::write(
        &not_a_plan,
        r#"{"schema_version": 17, "command": "doctor", "data": {}, "exit_code": 0}"#,
    )
    .unwrap();
    let (code, _) = ws.json(&[
        "snapshots",
        "generate",
        "--from-jigsaw-plan",
        not_a_plan.to_str().unwrap(),
    ]);
    assert_ne!(code, 0);
}

const TABLE_PINS: &str = r#"[host.pins]
source = "table"

[host.pins.releases."1.0.0"]
widget = "1.0.0"

[host.pins.releases."1.0.1"]
widget = "1.0.0"

[host.pins.releases."1.0.2"]
widget = "1.1.0"
"#;

#[test]
fn a_pin_table_replaces_the_pin_file() {
    let ws = workspace_with_pins(TABLE_PINS);
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    let (code, plan) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, PARTIAL_SUCCESS, "{plan}");
    assert_eq!(plan["data"]["releases"][2]["pins"]["widget"], "v1.1.0");
    let (code, generated) = ws.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
    assert_eq!(code, 0, "{generated}");
    let (code, again) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, 0, "{again}");
}

#[test]
fn a_release_missing_from_the_pin_table_fails_the_plan_naming_the_tag() {
    let table = TABLE_PINS.replace("[host.pins.releases.\"1.0.2\"]\nwidget = \"1.1.0\"\n", "");
    let ws = workspace_with_pins(&table);
    let assert = run([
        "--config-file-path",
        ws.config.to_str().unwrap(),
        "jigsaw",
        "plan",
        "--host",
        "queue",
        "--series",
        "1.0",
    ]);
    let output = assert.get_output().clone();
    assert_eq!(output.status.code(), Some(65));
    assert!(String::from_utf8(output.stderr).unwrap().contains("v1.0.2"));
}

#[test]
fn changing_the_pin_source_changes_the_cache_key() {
    let file = workspace();
    let table = workspace_with_pins(TABLE_PINS);
    let (_, from_file) = file.json(&["jigsaw", "plan", "--host", "queue", "--series", "1.0"]);
    let (_, from_table) = table.json(&["jigsaw", "plan", "--host", "queue", "--series", "1.0"]);
    assert_ne!(
        from_file["data"]["cache_key"],
        from_table["data"]["cache_key"]
    );
    assert_ne!(
        from_file["data"]["host_facts_digest"],
        from_table["data"]["host_facts_digest"]
    );
}

fn stock(ws: &Workspace, plan_path: &str) {
    ws.json(&plan_args(plan_path, &[]));
    let (code, generated) = ws.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
    assert_eq!(code, 0, "{generated}");
}

fn extract_widget_with_erlang_mk_manifests(ws: &Workspace) {
    let text = fs::read_to_string(&ws.config).unwrap();
    let flipped = text.replacen(
        "[[project]]\nname = \"widget\"\n",
        "[[project]]\nname = \"widget\"\nmanifest = \"erlang_mk\"\n",
        1,
    );
    assert_ne!(text, flipped);
    fs::write(&ws.config, flipped).unwrap();
}

#[test]
fn a_changed_extraction_setting_makes_the_stored_snapshots_stale_with_the_reason() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    stock(&ws, plan_path);

    extract_widget_with_erlang_mk_manifests(&ws);
    let (code, plan) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, PARTIAL_SUCCESS, "{plan}");
    let missing = plan["data"]["missing_snapshots"].as_array().unwrap();
    assert_eq!(missing.len(), 2, "{missing:?}");
    for row in missing {
        assert_eq!(row["project"], "widget");
        assert_eq!(row["reason"], "stale");
        assert!(
            row["detail"]
                .as_str()
                .unwrap()
                .contains("extraction settings")
        );
    }

    let (code, generated) = ws.json(&["snapshots", "generate", "--from-jigsaw-plan", plan_path]);
    assert_eq!(code, 0, "{generated}");
    let (code, again) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, 0, "{again}");
}

#[test]
fn refresh_stale_rebuilds_snapshots_made_with_other_extraction_settings() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    stock(&ws, plan_path);
    extract_widget_with_erlang_mk_manifests(&ws);

    let (code, generated) = ws.json(&[
        "snapshots",
        "generate",
        "--project",
        "widget",
        "--refresh-stale",
    ]);
    assert_eq!(code, 0, "{generated}");
    let (code, plan) = ws.json(&plan_args(plan_path, &[]));
    assert_eq!(code, 0, "{plan}");
}

fn stored_snapshots(ws: &Workspace) -> BTreeMap<String, String> {
    let root = ws.path("snapshots");
    let mut files = BTreeMap::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            let without_timestamp: Vec<&str> = text
                .lines()
                .filter(|line| !line.starts_with("# generated-at:"))
                .collect();
            let name = path.strip_prefix(&root).unwrap().display().to_string();
            files.insert(name, without_timestamp.join("\n"));
        }
    }
    files
}

#[test]
fn one_worker_and_many_workers_write_the_same_store() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    ws.json(&plan_args(plan_path, &[]));

    let generate = |jobs: &str| {
        let (code, generated) = ws.json(&[
            "snapshots",
            "generate",
            "--from-jigsaw-plan",
            plan_path,
            "--jobs",
            jobs,
        ]);
        assert_eq!(code, 0, "{generated}");
        generated
    };
    let serial = generate("1");
    let serial_store = stored_snapshots(&ws);
    assert_eq!(serial_store.len(), 6);
    fs::remove_dir_all(ws.path("snapshots")).unwrap();

    let parallel = generate("4");
    assert_eq!(stored_snapshots(&ws), serial_store);
    assert_eq!(parallel["data"]["generated"], serial["data"]["generated"]);
}

#[test]
fn a_snapshot_that_cannot_be_built_is_reported_and_the_others_are_still_written() {
    let ws = workspace();
    let plan_file = ws.path("plan.json");
    let plan_path = plan_file.to_str().unwrap();
    ws.json(&plan_args(plan_path, &[]));
    fs::remove_dir_all(ws.repos[1].path()).unwrap();

    let (code, generated) = ws.json(&[
        "snapshots",
        "generate",
        "--from-jigsaw-plan",
        plan_path,
        "--jobs",
        "4",
    ]);
    assert_ne!(code, 0, "{generated}");
    let failed = generated["data"]["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 2, "{generated}");
    assert!(failed.iter().all(|pin| pin["project"] == "widget"));
    assert_eq!(generated["data"]["generated"].as_array().unwrap().len(), 4);
    assert_eq!(stored_snapshots(&ws).len(), 4);
}

#[test]
fn a_worker_count_below_one_is_a_usage_error() {
    let ws = workspace();
    let (code, _) = ws.text(&["snapshots", "generate", "--jobs", "0", "--dry-run"]);
    assert_eq!(code, 2);
    let (code, _) = ws.text(&["snapshots", "generate", "--jobs", "many", "--dry-run"]);
    assert_eq!(code, 2);
}
