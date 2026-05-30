// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::model::names::{CommitSha, ModuleName, ProjectName, TagName};
use backhopper_core::model::snapshot::{Module, Snapshot, SnapshotHeader, state};
use time::OffsetDateTime;

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("p").unwrap(),
        tag: TagName::new("v1").unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
    }
}

fn module_with_behaviours(name: &str, behaviours: &[&str]) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    for b in behaviours {
        m.behaviours.push(ModuleName::new(*b).unwrap());
    }
    m
}

fn canonical(modules: Vec<Module>) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header(), modules, Vec::new()).into_canonical()
}

#[test]
fn implementers_of_finds_modules_declaring_the_behaviour() {
    let snap = canonical(vec![
        module_with_behaviours("rabbit_fifo", &["ra_machine"]),
        module_with_behaviours("rabbit_stream_coordinator", &["ra_machine"]),
        module_with_behaviours("rabbit_x", &["gen_server"]),
    ]);
    let ra_impls = snap.implementers_of(&ModuleName::new("ra_machine").unwrap());
    let names: Vec<&str> = ra_impls.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["rabbit_fifo", "rabbit_stream_coordinator"]);
}

#[test]
fn implementers_of_returns_empty_for_unused_behaviour() {
    let snap = canonical(vec![module_with_behaviours("a", &["gen_server"])]);
    assert!(
        snap.implementers_of(&ModuleName::new("nonexistent").unwrap())
            .is_empty()
    );
}

#[test]
fn implementers_of_handles_modules_with_multiple_behaviours() {
    let snap = canonical(vec![module_with_behaviours(
        "mc_amqp",
        &["mc", "gen_server"],
    )]);
    let mc_impls = snap.implementers_of(&ModuleName::new("mc").unwrap());
    let gs_impls = snap.implementers_of(&ModuleName::new("gen_server").unwrap());
    assert_eq!(mc_impls.len(), 1);
    assert_eq!(gs_impls.len(), 1);
}
