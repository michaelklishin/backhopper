// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `check commit` reads the Erlang fun bodies inside cuttlefish `.schema`
//! files through `backhopper-cuttlefish`, so an MFA reference there is
//! evaluated like any other. References outside a fun body are not.

use std::process::Command as Std;

use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, stdout};
use backhopper_test_support::{GitRepoFixture, write_config};

const DEMO_V1: &str = "-module(demo_mod).\n-export([greet/1]).\ngreet(Name) -> Name.\n";

// The mapping default outside any fun body names a module that must be
// ignored. Only the fun body's call is a real reference.
const SCHEMA_V1: &str = "\
{mapping, \"demo.thing\", \"demo_app.thing\", [{default, \"other_mod:ignored/1\"}]}.

{translation, \"demo_app.thing\",
fun(Conf) ->
    demo_mod:greet(Conf)
end}.
";

// The fun body now calls a function the snapshot does not export.
const SCHEMA_V2: &str = "\
{mapping, \"demo.thing\", \"demo_app.thing\", [{default, \"other_mod:ignored/1\"}]}.

{translation, \"demo_app.thing\",
fun(Conf) ->
    demo_mod:vanished(Conf)
end}.
";

fn build_repo() -> (GitRepoFixture, TempDir, String) {
    let workdir = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
    repo.write_file("src/demo_mod.erl", DEMO_V1);
    repo.write_file("deps/demo/priv/schema/demo.schema", SCHEMA_V1);
    repo.commit("v1");
    repo.tag("v1.0.0");
    repo.write_file("deps/demo/priv/schema/demo.schema", SCHEMA_V2);
    repo.commit("change schema fun body");
    let head = Std::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.dir.path())
        .output()
        .unwrap();
    let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    (repo, workdir, head_sha)
}

#[test]
fn schema_fun_body_reference_is_evaluated_and_non_body_reference_is_not() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let out = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "commit",
        "--project",
        "demo",
        "--tag",
        "v1.0.0",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "text",
        "--show-untracked-calls",
        &head_sha,
    ]));
    assert!(
        out.contains("1 tracked symbol referenced"),
        "the schema fun-body reference must be extracted and counted, got: {out}"
    );
    assert!(
        !out.contains("other_mod") && !out.contains("ignored"),
        "a reference outside a fun body must not be extracted, got: {out}"
    );
}
