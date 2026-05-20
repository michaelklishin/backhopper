// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;

use tempfile::TempDir;

use backhopper_core::suites::BuildSystem;

#[test]
fn detects_rebar3_when_rebar_config_present() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("rebar.config"), "{erl_opts, []}.").unwrap();
    assert_eq!(BuildSystem::detect(dir.path()), BuildSystem::Rebar3);
}

#[test]
fn detects_rebar3_when_rebar_config_script_present() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("rebar.config.script"), "[].").unwrap();
    assert_eq!(BuildSystem::detect(dir.path()), BuildSystem::Rebar3);
}

#[test]
fn detects_erlang_mk_when_file_present() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("erlang.mk"), "# erlang.mk").unwrap();
    assert_eq!(BuildSystem::detect(dir.path()), BuildSystem::ErlangMk);
}

#[test]
fn detects_erlang_mk_via_makefile_include() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Makefile"),
        "PROJECT = demo\ninclude erlang.mk",
    )
    .unwrap();
    assert_eq!(BuildSystem::detect(dir.path()), BuildSystem::ErlangMk);
}

#[test]
fn empty_repo_is_unknown() {
    let dir = TempDir::new().unwrap();
    assert_eq!(BuildSystem::detect(dir.path()), BuildSystem::Unknown);
}

#[test]
fn rebar3_takes_priority_over_erlang_mk() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("rebar.config"), "{erl_opts, []}.").unwrap();
    fs::write(dir.path().join("erlang.mk"), "# erlang.mk").unwrap();
    assert_eq!(BuildSystem::detect(dir.path()), BuildSystem::Rebar3);
}