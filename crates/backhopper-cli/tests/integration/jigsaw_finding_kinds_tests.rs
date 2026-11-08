// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use backhopper_test_support::{GitRepoFixture, toml_path};

use crate::helpers::jigsaw::{PARTIAL_SUCCESS, Workspace};

const RELEASES: [&str; 3] = ["v1.0.0", "v1.0.1", "v1.0.2"];
const PLUGIN_SOURCE: &str = "src/queue_extra.erl";
const PLUGIN_SCHEMA: &str = "priv/schema/queue_extra.schema";

// Release 2 is the build patch: what the plugin uses exists there, and an
// older release lacks it.
fn queue_app_source(release: usize) -> String {
    let exports = if release >= 1 {
        "-export([lookup/1, declare/2]).\n"
    } else {
        "-export([lookup/1]).\n"
    };
    let declare = if release >= 1 {
        "declare(Name, _) -> Name.\n"
    } else {
        ""
    };
    let types = if release >= 2 {
        "-export_type([state/0]).\n-type state() :: map().\n"
    } else {
        ""
    };
    format!("-module(queue_app).\n{exports}{types}lookup(Name) -> Name.\n{declare}")
}

fn queue_behaviour_source(release: usize) -> String {
    let optional = if release >= 2 {
        "-optional_callbacks([terminate/1]).\n"
    } else {
        ""
    };
    format!(
        "-module(queue_behaviour).\n-callback init(term()) -> ok.\n-callback terminate(term()) -> ok.\n{optional}"
    )
}

fn queue_header(release: usize) -> String {
    let limit = ["10", "50", "100"][release];
    let label = if release == 0 { "legacy" } else { "primary" };
    format!("-define(QUEUE_LIMIT, {limit}).\n-define(QUEUE_LABEL, \"{label}\").\n")
}

fn tls_partial(release: usize) -> String {
    let certfile = if release >= 2 {
        "{mapping, \"certfile\", \"ssl.certfile\", [{datatype, string}]}.\n"
    } else {
        ""
    };
    format!("{{mapping, \"cacertfile\", \"ssl.cacertfile\", [{{datatype, string}}]}}.\n{certfile}")
}

fn host_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("deps.mk", "# no dependencies\n");
    repo.write_file(
        "lib/queue_helper/src/queue_helper.erl",
        "-module(queue_helper).\n-export([run/0]).\nrun() -> ok.\n",
    );
    repo.write_file("lib/queue_helper/Makefile", "PROJECT = queue_helper\n");
    repo.write_file("lib/queue_app/Makefile", "PROJECT = queue_app\n");
    for (release, tag) in RELEASES.iter().enumerate() {
        repo.write_file(
            "lib/queue_app/src/queue_app.erl",
            &queue_app_source(release),
        );
        repo.write_file(
            "lib/queue_app/src/queue_behaviour.erl",
            &queue_behaviour_source(release),
        );
        repo.write_file(
            "lib/queue_app/include/queue_app.hrl",
            &queue_header(release),
        );
        repo.write_file(
            "lib/queue_app/priv/schema/tls_options.partial",
            &tls_partial(release),
        );
        if release >= 2 {
            repo.write_file(
                "lib/queue_app/src/queue_new.erl",
                "-module(queue_new).\n-export([ping/0]).\nping() -> ok.\n",
            );
        }
        let plugins = if release >= 1 {
            "PLUGINS = queue_app queue_helper\n"
        } else {
            "PLUGINS = queue_app\n"
        };
        repo.write_file("plugins.mk", plugins);
        repo.write_file("RELEASE", tag);
        repo.commit(tag);
        repo.tag(tag);
    }
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
    repo.tag("OTP-27.3");
    repo
}

fn config_text(dir: &Path, host: &Path, otp: &Path) -> String {
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
        otp = toml_path(otp),
    )
}

const PLUGIN_ERL: &str = "-module(queue_extra).
-behaviour(queue_behaviour).
-include_lib(\"queue_app/include/queue_app.hrl\").
-export([init/1, declare/1, ping/0, limit/0, label/0]).
-export_type([extra/0]).
-type extra() :: queue_app:state().

init(State) -> State.
declare(Name) -> queue_app:declare(Name, []).
ping() -> queue_new:ping().
limit() -> ?QUEUE_LIMIT.
label() -> ?QUEUE_LABEL.
";

fn write_plugin(dir: &Path) -> PathBuf {
    let root = dir.join("queue_extra");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("priv/schema")).unwrap();
    fs::write(
        root.join("Makefile"),
        "PROJECT = queue_extra\nDEPS = queue_app queue_helper\n",
    )
    .unwrap();
    fs::write(root.join(PLUGIN_SOURCE), PLUGIN_ERL).unwrap();
    fs::write(
        root.join(PLUGIN_SCHEMA),
        "{include_partial, {queue_app, \"tls_options\"}, [{exclude, [\"certfile\"]}]}.\n",
    )
    .unwrap();
    dir.to_owned()
}

fn checked() -> (Workspace, Value) {
    let (host, otp) = (host_repo(), otp_repo());
    let paths = (host.path().to_owned(), otp.path().to_owned());
    let ws = Workspace::new(vec![host, otp], |dir| config_text(dir, &paths.0, &paths.1));
    let plan = ws.stock();
    let plugins = write_plugin(&ws.path("plugins"));
    ws.remove_repositories();
    let (code, json) = ws.json(&[
        "jigsaw",
        "check",
        "--plan-file-path",
        plan.to_str().unwrap(),
        "--plugin-dir-path",
        plugins.to_str().unwrap(),
    ]);
    assert_eq!(code, PARTIAL_SUCCESS, "{json}");
    (ws, json["data"].clone())
}

fn entries<'a>(data: &'a Value, list: &str, kind: &str) -> Vec<&'a Value> {
    data[list]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == kind)
        .collect()
}

struct Expected {
    kind: &'static str,
    first: &'static str,
    last: &'static str,
    path: String,
    line: u64,
}

#[test]
fn every_finding_kind_names_its_releases_and_the_plugin_site() {
    let (_ws, data) = checked();
    let source = PLUGIN_SOURCE.to_owned();
    let schema = PLUGIN_SCHEMA.to_owned();
    let expected = [
        Expected {
            kind: "module_missing",
            first: "v1.0.0",
            last: "v1.0.1",
            path: source.clone(),
            line: 10,
        },
        Expected {
            kind: "function_missing",
            first: "v1.0.0",
            last: "v1.0.0",
            path: source.clone(),
            line: 9,
        },
        Expected {
            kind: "macro_changed",
            first: "v1.0.0",
            last: "v1.0.0",
            path: source.clone(),
            line: 11,
        },
        Expected {
            kind: "macro_changed",
            first: "v1.0.1",
            last: "v1.0.1",
            path: source.clone(),
            line: 11,
        },
        Expected {
            kind: "callback_required",
            first: "v1.0.0",
            last: "v1.0.1",
            path: source.clone(),
            line: 2,
        },
        Expected {
            kind: "type_missing",
            first: "v1.0.0",
            last: "v1.0.1",
            path: source.clone(),
            line: 6,
        },
        Expected {
            kind: "partial_key_missing",
            first: "v1.0.0",
            last: "v1.0.1",
            path: schema,
            line: 1,
        },
        Expected {
            kind: "app_missing",
            first: "v1.0.0",
            last: "v1.0.0",
            path: "Makefile".to_owned(),
            line: 1,
        },
    ];
    for want in expected {
        let range = serde_json::json!([{"first": want.first, "last": want.last}]);
        let found: Vec<&Value> = entries(&data, "findings", want.kind)
            .into_iter()
            .filter(|finding| finding["host_release_ranges"] == range)
            .collect();
        assert_eq!(found.len(), 1, "{} at {range}: {data}", want.kind);
        let finding = found[0];
        let sites = finding["sites"].as_array().unwrap();
        assert!(
            sites
                .iter()
                .any(|site| site["path"] == want.path.as_str() && site["line"] == want.line),
            "{} {sites:?}",
            want.kind
        );
    }
}

#[test]
fn a_string_literal_macro_that_differs_is_a_warning_not_a_finding() {
    let (_ws, data) = checked();
    assert!(entries(&data, "findings", "macro_text_changed").is_empty());
    let warnings = entries(&data, "warnings", "macro_text_changed");
    assert_eq!(warnings.len(), 1, "{data}");
    assert_eq!(
        warnings[0]["host_release_ranges"],
        serde_json::json!([{"first": "v1.0.0", "last": "v1.0.0"}])
    );
}
