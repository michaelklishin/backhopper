// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use backhopper_cli::commands::host_facts::HostFacts;
use backhopper_cli::commands::snapshots::build_host_snapshot_at_commit;
use backhopper_core::config::{Config, PinSource};
use backhopper_core::model::app_manifest::{ManifestDeps, ManifestSource, ManifestUnreadable};
use backhopper_core::model::names::{
    ApplicationName, HostFactsDigest, MakeVariableName, ProjectName, RelativePath, TagName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_roots::{MakefileVariable, ReleaseRoots};
use backhopper_core::snapshot::{format, parser};
use backhopper_git::GitRepo;
use backhopper_test_support::{GitRepoFixture, toml_path};

use crate::helpers::cli::run_succeeds;

const APP_A: &str = "\
-module(app_a).
-include_lib(\"app_b/include/app_b.hrl\").
-export([go/0]).
go() -> ?TIMEOUT.
";

const APP_B: &str = "-module(app_b).\n-export([run/0]).\nrun() -> ok.\n";

const APP_B_HEADER: &str = "\
-include_lib(\"kernel/include/file.hrl\").
-define(TIMEOUT, 1800000). %% thirty minutes
-define(IS_QUEUE(Q), is_record(Q, queue)).
-ifdef(OTP_RELEASE).
-define(LOGGER, logger).
-endif.
-record(queue, {name :: binary(), durable = false :: boolean(), arguments = []}).
";

const APP_A_SCHEMA: &str = "\
{mapping, \"app_a.port\", \"app_a.port\",
 [{datatype, integer}, {validators, [\"port\"]}]}.

{validator, \"port\", \"must be a valid port\",
 fun(P) -> P > 0 andalso P < 65536 end}.
";

const TLS_PARTIAL: &str = "\
{mapping, \"cacertfile\", \"ssl_options.cacertfile\", [{datatype, string}]}.
{mapping, \"verify\", \"ssl_options.verify\", [{datatype, {enum, [verify_peer, verify_none]}}]}.
";

const PIN_FILE: &str = "\
dep_ra = hex 2.17.1
dep_osiris = git_rmq osiris v1.13.1
";

fn build_fixture() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("lib/app_a/src/app_a.erl", APP_A);
    repo.write_file(
        "lib/app_a/Makefile",
        "PROJECT = app_a\nDEPS = app_b\nLOCAL_DEPS = ssl\nTEST_DEPS = meck\n",
    );
    repo.write_file("lib/app_a/priv/schema/app_a.schema", APP_A_SCHEMA);
    repo.write_file("lib/app_a/priv/schema/tls.partial", TLS_PARTIAL);
    repo.write_file("lib/app_a/priv/schema/notes.snippets", "# not a schema\n");
    repo.write_file("lib/app_b/src/app_b.erl", APP_B);
    repo.write_file("lib/app_b/include/app_b.hrl", APP_B_HEADER);
    repo.write_file(
        "lib/app_b/Makefile",
        "PROJECT = app_b\nDEPS = $(EXTRA_DEPS)\n",
    );
    repo.write_file("rabbitmq-components.mk", PIN_FILE);
    repo.write_file("plugins.mk", "PLUGINS := app_a \\\n  app_b\n");
    repo.commit("seed");
    repo.tag("v1.0.0");
    repo
}

fn write_config(workdir: &Path, repo: &Path) -> std::path::PathBuf {
    let snapshots = workdir.join("snapshots");
    fs::create_dir_all(&snapshots).unwrap();
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir = "{}"

[[project]]
name = "demo"
git_url = "{}"
layout = "multi_app"
app_roots = ["lib/*"]
excluded_subdirs = ["test"]
manifest = "erlang_mk"
"#,
        toml_path(&snapshots),
        toml_path(repo),
    );
    let path = workdir.join("backhopper.toml");
    fs::write(&path, body).unwrap();
    path
}

fn generated_text() -> String {
    let repo = build_fixture();
    let workdir = TempDir::new().unwrap();
    let cfg = write_config(workdir.path(), repo.path());
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    fs::read_to_string(workdir.path().join("snapshots/demo/v1.0.0.api.txt")).unwrap()
}

#[test]
fn generate_writes_format_5_with_the_current_extractor() {
    let text = generated_text();
    assert!(text.contains("# format-version: 5\n"), "{text}");
    assert!(
        text.contains(&format!(
            "# extractor-version: {}\n",
            backhopper_erlang::EXTRACTOR_VERSION
        )),
        "{text}"
    );
}

#[test]
fn modules_carry_their_app_and_path() {
    let text = generated_text();
    assert!(
        text.contains("module app_a\n  path lib/app_a/src/app_a.erl\n  app app_a\n"),
        "{text}"
    );
    assert!(
        text.contains("module app_b\n  path lib/app_b/src/app_b.erl\n  app app_b\n"),
        "{text}"
    );
}

#[test]
fn each_app_has_a_manifest_section_read_from_its_makefile() {
    let text = generated_text();
    assert!(
        text.contains("app app_a\n  manifest erlang_mk\n  requires app_b\n  requires ssl\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "app app_b\n  manifest erlang_mk\n  requires_unreadable unexpanded_variable EXTRA_DEPS\n"
        ),
        "{text}"
    );
    assert!(!text.contains("meck"), "TEST_DEPS must not be recorded");
}

#[test]
fn the_header_records_its_app_includes_macros_and_record_defaults() {
    let text = generated_text();
    assert!(
        text.contains(
            "header lib/app_b/include/app_b.hrl\n  app app_b\n  include kernel include/file.hrl\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("    field durable :: boolean() = false\n"),
        "{text}"
    );
    assert!(text.contains("    field arguments = []\n"), "{text}");
    assert!(
        text.contains("  define IS_QUEUE/1 is_record(Q, queue)\n"),
        "{text}"
    );
    assert!(text.contains("  define LOGGER ?\n"), "{text}");
    assert!(text.contains("  define TIMEOUT 1800000\n"), "{text}");
}

#[test]
fn the_schema_and_partial_list_their_keys_and_validators() {
    let text = generated_text();
    assert!(
        text.contains("schema lib/app_a/priv/schema/app_a.schema\n  app app_a\n  mapping app_a.port\n  validator port\n"),
        "{text}"
    );
    assert!(
        text.contains("partial lib/app_a/priv/schema/tls.partial\n  app app_a\n  name tls\n  mapping cacertfile\n  mapping verify\n"),
        "{text}"
    );
    assert!(!text.contains("notes.snippets"));
}

#[test]
fn the_generated_snapshot_parses_and_is_stable() {
    let text = generated_text();
    let snap = parser::parse(&text).unwrap();
    assert_eq!(format::to_string(&snap).unwrap(), text);
    assert_eq!(snap.apps().len(), 2);
    assert!(
        snap.apps()
            .iter()
            .all(|a| a.source == ManifestSource::ErlangMk)
    );
}

#[test]
fn an_app_without_a_manifest_file_is_marked_missing() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "lib/app_a/src/app_a.erl",
        APP_B.replace("app_b", "app_a").as_str(),
    );
    repo.commit("seed");
    repo.tag("v1.0.0");
    let workdir = TempDir::new().unwrap();
    let cfg = write_config(workdir.path(), repo.path());
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let text = fs::read_to_string(workdir.path().join("snapshots/demo/v1.0.0.api.txt")).unwrap();
    let snap = parser::parse(&text).unwrap();
    assert_eq!(
        snap.apps()[0].requires,
        ManifestDeps::Unreadable(ManifestUnreadable::Missing)
    );
}

fn host_facts() -> HostFacts {
    HostFacts {
        pins: PinSource::ErlangMk {
            file_path: RelativePath::new("rabbitmq-components.mk").unwrap(),
        },
        release_roots: ReleaseRoots::Both {
            apps: NonEmpty::new(ApplicationName::new("app_a").unwrap(), []),
            variable: MakefileVariable {
                file_path: RelativePath::new("plugins.mk").unwrap(),
                variable: MakeVariableName::new("PLUGINS").unwrap(),
            },
        },
        digest: HostFactsDigest::new("c".repeat(64)).unwrap(),
    }
}

#[test]
fn a_host_snapshot_records_release_roots_pins_and_the_facts_digest() {
    let repo = build_fixture();
    let workdir = TempDir::new().unwrap();
    let cfg = write_config(workdir.path(), repo.path());
    let config = Config::load(&cfg).unwrap();
    let project = config.project(&ProjectName::new("demo").unwrap()).unwrap();
    let git = GitRepo::open(repo.path()).unwrap();
    let tag = TagName::new("v1.0.0").unwrap();
    let commit = git.resolve_tag(&tag).unwrap();
    let snapshot =
        build_host_snapshot_at_commit(project, &git, &commit, &tag, &host_facts()).unwrap();
    let text = format::to_string(&snapshot).unwrap();
    assert!(
        text.contains("# dep-pin: osiris = git_rmq v1.13.1\n"),
        "{text}"
    );
    assert!(text.contains("# dep-pin: ra = hex 2.17.1\n"), "{text}");
    assert!(
        text.contains("# release-root: app_a\n# release-root: app_b\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!("# host-facts: {}\n", "c".repeat(64))),
        "{text}"
    );
    assert_eq!(
        format::to_string(&parser::parse(&text).unwrap()).unwrap(),
        text
    );
}

#[test]
fn a_host_snapshot_fails_when_the_roots_variable_cannot_be_read() {
    let repo = build_fixture();
    let workdir = TempDir::new().unwrap();
    let cfg = write_config(workdir.path(), repo.path());
    let config = Config::load(&cfg).unwrap();
    let project = config.project(&ProjectName::new("demo").unwrap()).unwrap();
    let git = GitRepo::open(repo.path()).unwrap();
    let tag = TagName::new("v1.0.0").unwrap();
    let commit = git.resolve_tag(&tag).unwrap();
    let mut facts = host_facts();
    facts.release_roots = ReleaseRoots::Variable(MakefileVariable {
        file_path: RelativePath::new("plugins.mk").unwrap(),
        variable: MakeVariableName::new("RELEASE_APPS").unwrap(),
    });
    let error = build_host_snapshot_at_commit(project, &git, &commit, &tag, &facts).unwrap_err();
    assert!(error.to_string().contains("RELEASE_APPS"), "{error}");
}

#[test]
fn a_host_snapshot_fails_when_the_pin_file_is_absent() {
    let repo = build_fixture();
    let workdir = TempDir::new().unwrap();
    let cfg = write_config(workdir.path(), repo.path());
    let config = Config::load(&cfg).unwrap();
    let project = config.project(&ProjectName::new("demo").unwrap()).unwrap();
    let git = GitRepo::open(repo.path()).unwrap();
    let tag = TagName::new("v1.0.0").unwrap();
    let commit = git.resolve_tag(&tag).unwrap();
    let mut facts = host_facts();
    facts.pins = PinSource::ErlangMk {
        file_path: RelativePath::new("deps.mk").unwrap(),
    };
    let error = build_host_snapshot_at_commit(project, &git, &commit, &tag, &facts).unwrap_err();
    assert!(error.to_string().contains("deps.mk"), "{error}");
}

fn write_single_app_config(workdir: &Path, repo: &Path, name: &str) -> std::path::PathBuf {
    let snapshots = workdir.join("snapshots");
    fs::create_dir_all(&snapshots).unwrap();
    let body = format!(
        "config_version = 1\n\n[defaults]\nsnapshot_dir = \"{}\"\n\n[[project]]\nname = \"{name}\"\ngit_url = \"{}\"\n",
        toml_path(&snapshots),
        toml_path(repo),
    );
    let path = workdir.join("backhopper.toml");
    fs::write(&path, body).unwrap();
    path
}

fn generate_single_app(repo: &GitRepoFixture, name: &str) -> String {
    let workdir = TempDir::new().unwrap();
    let cfg = write_single_app_config(workdir.path(), repo.path(), name);
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        name,
    ]);
    fs::read_to_string(
        workdir
            .path()
            .join(format!("snapshots/{name}/v1.0.0.api.txt")),
    )
    .unwrap()
}

const RA_APP_SRC: &str = "\
{application, ra,
 [{description, \"Raft library\"},
  {vsn, \"2.17.1\"},
  {applications, [kernel, stdlib, aten, gen_batch_server, seshat]},
  {included_applications, [sasl]},
  {mod, {ra_app, []}}
 ]}.
";

#[test]
fn a_single_app_project_takes_its_app_from_the_app_src_name() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "src/ra.erl",
        "-module(ra).\n-export([start/0]).\nstart() -> ok.\n",
    );
    repo.write_file("src/ra.app.src", RA_APP_SRC);
    repo.write_file("include/ra.hrl", "-define(RA_PROTO_VERSION, 1).\n");
    repo.commit("seed");
    repo.tag("v1.0.0");
    let text = generate_single_app(&repo, "demo");
    assert!(
        text.contains("module ra\n  path src/ra.erl\n  app ra\n"),
        "{text}"
    );
    assert!(
        text.contains("app ra\n  manifest app_src\n  requires aten\n  requires gen_batch_server\n  requires kernel\n  requires sasl\n  requires seshat\n  requires stdlib\n"),
        "{text}"
    );
    assert!(text.contains("header include/ra.hrl\n  app ra\n"), "{text}");
    assert!(text.contains("  define RA_PROTO_VERSION 1\n"), "{text}");
}

#[test]
fn a_single_app_project_without_a_manifest_takes_the_project_name() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "src/osiris.erl",
        "-module(osiris).\n-export([start/0]).\nstart() -> ok.\n",
    );
    repo.commit("seed");
    repo.tag("v1.0.0");
    let text = generate_single_app(&repo, "osiris");
    assert!(
        text.contains("module osiris\n  path src/osiris.erl\n  app osiris\n"),
        "{text}"
    );
    assert!(
        text.contains("app osiris\n  manifest app_src\n  requires_unreadable missing\n"),
        "{text}"
    );
}

#[test]
fn a_multi_app_project_reads_app_src_manifests_by_default() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "lib/kernel/src/file.erl",
        "-module(file).\n-export([open/2]).\nopen(_, _) -> ok.\n",
    );
    repo.write_file(
        "lib/kernel/src/kernel.app.src",
        "{application, kernel, [{applications, []}, {included_applications, []}]}.\n",
    );
    repo.write_file(
        "lib/stdlib/src/lists.erl",
        "-module(lists).\n-export([map/2]).\nmap(_, _) -> [].\n",
    );
    repo.write_file(
        "lib/stdlib/src/stdlib.app.src",
        "{application, stdlib, [{applications, [kernel]}]}.\n",
    );
    repo.commit("seed");
    repo.tag("v1.0.0");
    let workdir = TempDir::new().unwrap();
    let snapshots = workdir.path().join("snapshots");
    fs::create_dir_all(&snapshots).unwrap();
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(
        &cfg,
        format!(
            "config_version = 1\n\n[defaults]\nsnapshot_dir = \"{}\"\n\n[[project]]\nname = \"otp\"\ngit_url = \"{}\"\nlayout = \"multi_app\"\napp_roots = [\"lib/*\"]\n",
            toml_path(&snapshots),
            toml_path(repo.path()),
        ),
    )
    .unwrap();
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "otp",
    ]);
    let text = fs::read_to_string(snapshots.join("otp/v1.0.0.api.txt")).unwrap();
    assert!(
        text.contains("app kernel\n  manifest app_src\n\n"),
        "{text}"
    );
    assert!(
        text.contains("app stdlib\n  manifest app_src\n  requires kernel\n"),
        "{text}"
    );
}
