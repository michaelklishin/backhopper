// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use time::OffsetDateTime;

use backhopper_core::compat::scope::{PinScope, UntrackedTally, parse_module_names};
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::{
    FunArity, Module, Snapshot, SnapshotHeader, Visibility, state,
};

fn header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src/**/*.erl".into()],
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
    }
}

fn module(name: &str) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.visibility = Visibility::Public;
    m.exports.push(FunArity {
        name: FunctionName::new("noop").unwrap(),
        arity: Arity::new(0),
    });
    m
}

fn snapshot_with(project: &str, modules: &[&str]) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(
        header(project, "v1.0.0"),
        modules.iter().map(|n| module(n)).collect(),
        vec![],
    )
    .into_canonical()
}

#[test]
fn scope_treats_every_module_in_snapshot_as_tracked() {
    let snap = snapshot_with("ra", &["ra", "ra_log", "ra_machine"]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    assert!(scope.contains_module(&ModuleName::new("ra").unwrap()));
    assert!(scope.contains_module(&ModuleName::new("ra_log").unwrap()));
    assert!(!scope.contains_module(&ModuleName::new("rabbit_misc").unwrap()));
    assert!(!scope.contains_module(&ModuleName::new("lists").unwrap()));
}

#[test]
fn extra_modules_are_added_to_scope() {
    let snap = snapshot_with("ra", &["ra"]);
    let extras = vec![ModuleName::new("ra_helper").unwrap()];
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, extras);
    assert!(scope.contains_module(&ModuleName::new("ra_helper").unwrap()));
}

#[test]
fn rewrite_path_strips_deps_project_prefix() {
    let snap = snapshot_with("ra", &["ra"]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let rewritten = scope.rewrite_path(Path::new("deps/ra/src/ra_log.erl"));
    assert_eq!(rewritten, Some(Path::new("src/ra_log.erl")));
}

#[test]
fn rewrite_path_passes_project_local_paths_through() {
    let snap = snapshot_with("ra", &["ra"]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let rewritten = scope.rewrite_path(Path::new("src/ra_log.erl"));
    assert_eq!(rewritten, Some(Path::new("src/ra_log.erl")));
}

#[test]
fn rewrite_path_rejects_other_projects_deps_dirs() {
    let snap = snapshot_with("ra", &["ra"]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    assert_eq!(scope.rewrite_path(Path::new("deps/rabbit/Makefile")), None);
    assert_eq!(
        scope.rewrite_path(Path::new("deps/khepri/src/khepri.erl")),
        None
    );
}

#[test]
fn default_scope_accepts_any_path_but_no_modules() {
    let scope = PinScope::default();
    assert!(!scope.contains_module(&ModuleName::new("ra").unwrap()));
    assert_eq!(
        scope.rewrite_path(Path::new("anything/else.erl")),
        Some(Path::new("anything/else.erl"))
    );
}

#[test]
fn parse_module_names_drops_invalid_strings() {
    let inputs: [String; 4] = [
        "ra".into(),
        "Bad Module".into(),
        "ra_log".into(),
        "9digits".into(),
    ];
    let parsed = parse_module_names(inputs.iter());
    let strings: Vec<String> = parsed.iter().map(|m| m.to_string()).collect();
    assert_eq!(strings, vec!["ra", "ra_log"]);
}

#[test]
fn untracked_tally_counts_repeats() {
    let mut t = UntrackedTally::default();
    let m1 = ModuleName::new("lists").unwrap();
    let m2 = ModuleName::new("maps").unwrap();
    t.record(m1.clone());
    t.record(m1.clone());
    t.record(m2);
    let map = t.into_map();
    assert_eq!(
        map.get(&ModuleName::new("lists").unwrap()).copied(),
        Some(2)
    );
    assert_eq!(map.get(&ModuleName::new("maps").unwrap()).copied(), Some(1));
}