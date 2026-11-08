// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin;
use backhopper_driver::types::{HostName, HostSeries, JigsawFindingKind};
use backhopper_driver::{Backhopper, ErrorKind};
use serde_json::Value;

use backhopper_test_support::{GitRepoFixture, toml_path};

use crate::helpers::jigsaw::{PARTIAL_SUCCESS, Workspace};

const DATA_ERROR: i32 = 65;
const IO_ERROR: i32 = 74;

const QUEUE_APP: &str = "lib/queue_app";

fn host_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "lib/queue_app/Makefile",
        "PROJECT = queue_app\nDEPS = widget\n",
    );
    repo.write_file(
        "lib/queue_app/src/queue_app.erl",
        "-module(queue_app).\n-export([lookup/1, declare/2]).\nlookup(Name) -> Name.\ndeclare(Name, _) -> Name.\n",
    );
    repo.write_file(
        "lib/queue_app/priv/schema/queue_app.schema",
        "{mapping, \"queue.limit\", \"queue_app.limit\", [{datatype, integer}]}.\n",
    );
    repo.write_file("plugins.mk", "PLUGINS = queue_app\n");
    repo.write_file("deps.mk", "dep_widget = hex 1.0.0\n");
    for release in ["v1.0.0", "v1.0.1", "v1.0.2"] {
        let record = if release == "v1.0.2" {
            "-record(queue, {name, durable = false, owner}).\n"
        } else {
            "-record(queue, {name, durable = false}).\n"
        };
        repo.write_file(
            &format!("{QUEUE_APP}/include/queue_app.hrl"),
            &format!("{record}-define(QUEUE_LIMIT, 100).\n"),
        );
        if release != "v1.0.0" {
            repo.write_file(
                &format!("{QUEUE_APP}/priv/schema/tls_options.partial"),
                "{mapping, \"cacertfile\", \"ssl.cacertfile\", [{datatype, string}]}.\n",
            );
        }
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
    repo
}

fn otp_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "lib/kernel/src/kernel.erl",
        "-module(kernel).\n-export([stop/0]).\nstop() -> ok.\n",
    );
    repo.write_file(
        "lib/kernel/src/kernel.app.src",
        "{application, kernel, [{vsn, \"1\"}, {applications, []}]}.\n",
    );
    repo.commit("otp");
    for tag in ["OTP-27.3", "OTP-27.3.1"] {
        repo.tag(tag);
    }
    repo
}

fn workspace() -> Workspace {
    let (host, widget, otp) = (host_repo(), widget_repo(), otp_repo());
    let paths = (
        host.path().to_owned(),
        widget.path().to_owned(),
        otp.path().to_owned(),
    );
    Workspace::new(vec![host, widget, otp], |dir| {
        config_text(dir, &paths.0, &paths.1, &paths.2)
    })
}

fn config_text(dir: &Path, host: &Path, widget: &Path, otp: &Path) -> String {
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

[host.pins]
source = "erlang_mk"
file_path = "deps.mk"

[host.release_roots]
makefile_variable = {{ file_path = "plugins.mk", variable = "PLUGINS" }}

[host.erlang_support]
default = "[27.0,29.0)"
"#,
        snapshots = toml_path(&dir.join("snapshots")),
        host = toml_path(host),
        widget = toml_path(widget),
        otp = toml_path(otp),
    )
}

const EXTRA_SOURCE: &str = "-module(queue_extra).
-include_lib(\"queue_app/include/queue_app.hrl\").
-export([run/1, limit/0, build/1, dynamic/1, missing/0, internal/0]).

run(Name) -> queue_app:lookup(Name).
limit() -> ?QUEUE_LIMIT.
build(Name) -> #queue{name = Name}.
dynamic(M) -> M:start(), M:stop().
missing() -> queue_app:nosuch(1).
internal() -> queue_extra:run(1).
";

fn write_plugin(dir: &Path) {
    let root = dir.join("queue_extra");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("priv/schema")).unwrap();
    fs::write(
        root.join("Makefile"),
        "PROJECT = queue_extra\nDEPS = queue_app\n",
    )
    .unwrap();
    fs::write(root.join("src/queue_extra.erl"), EXTRA_SOURCE).unwrap();
    fs::write(
        root.join("priv/schema/queue_extra.schema"),
        "{include_partial, {queue_app, \"tls_options\"}, []}.\n",
    )
    .unwrap();
}

fn stocked_workspace() -> (Workspace, PathBuf) {
    let ws = workspace();
    let plan = ws.stock();
    (ws, plan)
}

fn check_args<'a>(plan: &'a str, plugins: &'a str) -> Vec<&'a str> {
    vec![
        "jigsaw",
        "check",
        "--plan-file-path",
        plan,
        "--plugin-dir-path",
        plugins,
    ]
}

fn findings<'a>(data: &'a Value, kind: &str) -> Vec<&'a Value> {
    data["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["kind"] == kind)
        .collect()
}

#[test]
fn check_reports_the_record_and_the_partial_and_never_opens_a_repository() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    ws.remove_repositories();

    let (code, json) = ws.json(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, PARTIAL_SUCCESS, "{json}");
    let data = &json["data"];
    assert_eq!(data["clean"], false);
    assert_eq!(data["build_release"], "v1.0.2");
    assert_eq!(data["host_releases"].as_array().unwrap().len(), 3);

    let record = findings(data, "record_fields_changed");
    assert_eq!(record.len(), 1, "{data}");
    assert_eq!(
        record[0]["host_release_ranges"],
        serde_json::json!([{"first": "v1.0.0", "last": "v1.0.1"}])
    );
    assert_eq!(
        record[0]["symbol"],
        "#queue{} in queue_app/include/queue_app.hrl"
    );

    let partial = findings(data, "partial_missing");
    assert_eq!(partial.len(), 1, "{data}");
    assert_eq!(
        partial[0]["host_release_ranges"],
        serde_json::json!([{"first": "v1.0.0", "last": "v1.0.0"}])
    );
    assert_eq!(data["findings"].as_array().unwrap().len(), 2, "{data}");

    let tallies = &data["tallies"];
    assert_eq!(tallies["not_checked"]["unresolved_at_build"], 1);
    assert_eq!(tallies["not_checked"]["dynamic"], 2);
    assert_eq!(tallies["not_checked"]["internal"], 1);
}

#[test]
fn check_lists_the_sites_it_could_not_compare_with_file_and_line() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let (_, json) = ws.json(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    let data = &json["data"];
    let expected = serde_json::json!([
        {"reason": "dynamic", "symbol": "_:start/0", "app": "queue_extra", "path": "src/queue_extra.erl", "line": 8},
        {"reason": "dynamic", "symbol": "_:stop/0", "app": "queue_extra", "path": "src/queue_extra.erl", "line": 8},
        {"reason": "unresolved_at_build", "symbol": "queue_app:nosuch/1", "app": "queue_extra", "path": "src/queue_extra.erl", "line": 9},
    ]);
    assert_eq!(data["unchecked"], expected, "{data}");
    for reason in ["dynamic", "unresolved_at_build"] {
        let listed = data["unchecked"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|site| site["reason"] == reason)
            .count();
        assert_eq!(data["tallies"]["not_checked"][reason], listed, "{reason}");
    }

    let (_, out) = ws.text(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert!(
        out.contains("queue_extra/src/queue_extra.erl:9  queue_app:nosuch/1"),
        "{out}"
    );
    assert!(
        out.contains("queue_extra/src/queue_extra.erl:8  _:start/0"),
        "{out}"
    );
}

#[test]
fn a_clean_plugin_set_exits_zero() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    let root = plugins.join("queue_clean");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Makefile"),
        "PROJECT = queue_clean\nDEPS = queue_app\n",
    )
    .unwrap();
    fs::write(
        root.join("src/queue_clean.erl"),
        "-module(queue_clean).\n-export([run/1]).\nrun(Name) -> queue_app:lookup(Name).\n",
    )
    .unwrap();
    let (code, json) = ws.json(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["data"]["clean"], true);
    assert_eq!(json["data"]["tallies"]["checked"], 2);
}

#[test]
fn a_test_directory_is_never_read() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    let root = plugins.join("queue_clean");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("test")).unwrap();
    fs::write(
        root.join("Makefile"),
        "PROJECT = queue_clean\nDEPS = queue_app\n",
    )
    .unwrap();
    fs::write(
        root.join("src/queue_clean.erl"),
        "-module(queue_clean).\n-export([run/1]).\nrun(Name) -> queue_app:lookup(Name).\n",
    )
    .unwrap();
    fs::write(
        root.join("test/queue_clean_SUITE.erl"),
        "-module(queue_clean_SUITE).\nall() -> queue_app:removed_long_ago(1), not valid erlang (\n",
    )
    .unwrap();
    let (code, json) = ws.json(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, 0, "{json}");
    assert!(json["data"]["findings"].as_array().unwrap().is_empty());
}

#[test]
fn host_copies_in_deps_are_left_out_and_the_host_findings_remain() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let copy = plugins.join("queue_extra/deps/queue_app");
    fs::create_dir_all(copy.join("src")).unwrap();
    fs::write(copy.join("Makefile"), "PROJECT = queue_app\n").unwrap();
    fs::write(
        copy.join("src/queue_app.erl"),
        "-module(queue_app).\n-export([lookup/1, nosuch/1]).\nlookup(N) -> N.\nnosuch(N) -> N.\n",
    )
    .unwrap();
    let (code, json) = ws.json(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, PARTIAL_SUCCESS, "{json}");
    let data = &json["data"];
    assert_eq!(data["host_provided"], serde_json::json!(["queue_app"]));
    assert_eq!(findings(data, "record_fields_changed").len(), 1);
    assert_eq!(data["tallies"]["not_checked"]["unresolved_at_build"], 1);
}

#[test]
fn an_unstocked_plan_exits_with_the_data_error_code() {
    let ws = workspace();
    let plan = ws.path("plan.json");
    ws.json(&[
        "jigsaw",
        "plan",
        "--host",
        "queue",
        "--series",
        "1.0",
        "--plan-file-path",
        plan.to_str().unwrap(),
    ]);
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let (code, _) = ws.text(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, DATA_ERROR);
}

#[test]
fn the_plan_arguments_form_lists_tags_from_the_store() {
    let (ws, _plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    ws.remove_repositories();
    let (code, json) = ws.json(&[
        "jigsaw",
        "check",
        "--host",
        "queue",
        "--series",
        "1.0",
        "--plugin-dir-path",
        plugins.to_str().unwrap(),
    ]);
    assert_eq!(code, PARTIAL_SUCCESS, "{json}");
    assert_eq!(json["data"]["tag_source"], "store");
    assert_eq!(json["data"]["host_releases"].as_array().unwrap().len(), 3);
}

#[test]
fn a_directory_without_apps_is_an_error() {
    let (ws, plan) = stocked_workspace();
    let empty = ws.path("empty");
    fs::create_dir_all(&empty).unwrap();
    let (code, _) = ws.text(&check_args(plan.to_str().unwrap(), empty.to_str().unwrap()));
    assert_eq!(code, DATA_ERROR);
}

#[test]
fn the_text_output_names_the_findings_their_releases_and_the_counts() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let (code, text) = ws.text(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, PARTIAL_SUCCESS, "{text}");
    assert!(text.contains("2 findings"), "{text}");
    assert!(text.contains("record_fields_changed"), "{text}");
    assert!(text.contains("host releases: v1.0.0 .. v1.0.1"), "{text}");
    assert!(text.contains("partial_missing"), "{text}");
    assert!(text.contains("src/queue_extra.erl:7"), "{text}");
    assert!(text.contains("not resolved at v1.0.2: 1"), "{text}");
}

#[test]
fn a_directory_that_cannot_be_read_exits_with_the_io_error_code() {
    let (ws, plan) = stocked_workspace();
    let missing = ws.path("missing");
    let (code, _) = ws.text(&check_args(
        plan.to_str().unwrap(),
        missing.to_str().unwrap(),
    ));
    assert_eq!(code, IO_ERROR);
}

#[test]
fn the_json_payload_carries_cells_and_limits_and_no_span_flag() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let (code, json) = ws.json(&check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    assert_eq!(code, PARTIAL_SUCCESS, "{json}");
    let data = &json["data"];
    assert_eq!(data["clean"], false);
    assert_eq!(data["limits"].as_array().unwrap().len(), 3);
    assert!(data["withheld"].is_array());
    let hosts = data["host_releases"].as_array().unwrap().len();
    let tags = data["otp_tags"].as_array().unwrap().len();
    for row in data["apps"].as_array().unwrap() {
        assert_eq!(row["cells"].as_array().unwrap().len(), hosts * tags);
    }
    assert!(data["findings"][0].get("everywhere").is_none());
}

#[test]
fn the_markdown_output_has_a_matrix_table_and_the_references_line() {
    let (ws, plan) = stocked_workspace();
    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let mut args = vec!["--formatter", "markdown"];
    args.extend(check_args(
        plan.to_str().unwrap(),
        plugins.to_str().unwrap(),
    ));
    let (code, text) = ws.text(&args);
    assert_eq!(code, PARTIAL_SUCCESS, "{text}");
    assert!(text.starts_with("## jigsaw check: "), "{text}");
    assert!(text.contains("| app | "), "{text}");
    assert!(
        text.contains("distinct references to the host and OTP"),
        "{text}"
    );
}

fn driver_for(ws: &Workspace) -> Backhopper {
    let mut driver = Backhopper::with_binary_path(cargo_bin("backhopper"));
    driver.options_mut().config_path = Some(ws.config.clone());
    driver
}

fn queue_series() -> (HostName, HostSeries) {
    (HostName::new("queue").unwrap(), "1.0".parse().unwrap())
}

#[test]
fn the_driver_plans_generates_and_checks_through_the_real_binary() {
    let ws = workspace();
    let driver = driver_for(&ws);
    let plan_path = ws.path("plan.json");
    let (host, series) = queue_series();
    let plan = driver
        .jigsaw()
        .plan()
        .host(host)
        .series(series)
        .plan_file_path(&plan_path)
        .run()
        .expect("missing snapshots exit with the partial success code");
    assert!(!plan.missing_snapshots.is_empty());

    let plugins = ws.path("plugins");
    write_plugin(&plugins);
    let unstocked = driver
        .jigsaw()
        .check()
        .plan_file(&plan_path)
        .plugin_dir(&plugins)
        .run()
        .expect_err("an unstocked plan is an error");
    assert_eq!(unstocked.kind(), ErrorKind::ToolError);

    let (code, generated) = ws.json(&[
        "snapshots",
        "generate",
        "--from-jigsaw-plan",
        plan_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{generated}");
    let report = driver
        .jigsaw()
        .check()
        .plan_file(&plan_path)
        .plugin_dir(&plugins)
        .run()
        .expect("findings exit with the partial success code");
    assert!(!report.clean);
    let kinds: Vec<JigsawFindingKind> = report.findings.iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&JigsawFindingKind::RecordFieldsChanged));
    assert!(kinds.contains(&JigsawFindingKind::PartialMissing));
    for row in &report.apps {
        assert_eq!(
            row.cells.len(),
            report.host_releases.len() * report.otp_tags.len()
        );
    }

    let (host, series) = queue_series();
    let arguments = driver
        .jigsaw()
        .plan()
        .host(host)
        .series(series)
        .into_arguments();
    let from_store = driver
        .jigsaw()
        .check()
        .plan_arguments(arguments)
        .plugin_dir(&plugins)
        .run()
        .unwrap();
    assert_eq!(from_store.findings, report.findings);
}
