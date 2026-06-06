// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use tempfile::TempDir;
use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, DependencyName, DependencyVersion, FunctionName, MacroName, ModuleName,
    ProjectName, TagName,
};
use backhopper_core::model::snapshot::{
    Module, Provenance, Snapshot, SnapshotHeader, VendoredDep, VendoredDepSource,
    VersionedMachineVersion, WireConstantBinding, WireValue, state,
};
use backhopper_core::store::SnapshotStore;

fn header_with_dep_pins() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("rabbitmq-server").unwrap(),
        tag: TagName::new("v4.0.5").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["deps".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: vec![
            VendoredDep {
                name: DependencyName::new("ra").unwrap(),
                version: DependencyVersion::new("2.17.0").unwrap(),
                source: VendoredDepSource::Hex,
            },
            VendoredDep {
                name: DependencyName::new("cowboy").unwrap(),
                version: DependencyVersion::new("2.13.0").unwrap(),
                source: VendoredDepSource::GitRmq,
            },
        ],
    }
}

fn rabbit_fifo_with_machine_version() -> Module {
    let mut m = Module::new(ModuleName::new("rabbit_fifo").unwrap());
    m.versioned_machine_version = Some(VersionedMachineVersion {
        function: FunctionName::from_str("version").unwrap(),
        arity: Arity::new(0),
        value: Some(7),
        provenance: Provenance::MacroBody {
            macro_name: MacroName::new("MACHINE_VERSION").unwrap(),
            defined_in: Some("deps/rabbit/include/rabbit_fifo.hrl".into()),
        },
    });
    m.wire_constants = vec![WireConstantBinding {
        macro_name: MacroName::new("RA_PROTO_VERSION").unwrap(),
        value: WireValue::U64(1),
        defined_in: None,
    }];
    m
}

fn snapshot_with_new_fields() -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(
        header_with_dep_pins(),
        vec![rabbit_fifo_with_machine_version()],
        vec![],
    )
    .into_canonical()
}

#[test]
fn store_round_trips_snapshot_with_versioned_machine_version() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let snap = snapshot_with_new_fields();
    store.write(&snap).unwrap();
    let project = snap.header().project.clone();
    let tag = snap.header().tag.clone();
    let back = store.read(&project, &tag).unwrap();
    assert_eq!(snap, back);
    let m = back
        .module_named(&ModuleName::new("rabbit_fifo").unwrap())
        .unwrap();
    let v = m.versioned_machine_version.as_ref().unwrap();
    assert_eq!(v.value, Some(7));
    assert!(matches!(
        &v.provenance,
        Provenance::MacroBody { macro_name, .. } if macro_name.as_str() == "MACHINE_VERSION"
    ));
}

#[test]
fn store_round_trips_snapshot_with_wire_constants() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let snap = snapshot_with_new_fields();
    store.write(&snap).unwrap();
    let back = store
        .read(&snap.header().project, &snap.header().tag)
        .unwrap();
    let m = back
        .module_named(&ModuleName::new("rabbit_fifo").unwrap())
        .unwrap();
    assert_eq!(m.wire_constants.len(), 1);
    let wc = &m.wire_constants[0];
    assert_eq!(wc.macro_name.as_str(), "RA_PROTO_VERSION");
    assert_eq!(wc.value, WireValue::U64(1));
}

#[test]
fn store_round_trips_snapshot_with_dep_pins_in_alphabetical_order() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let snap = snapshot_with_new_fields();
    store.write(&snap).unwrap();
    let back = store
        .read(&snap.header().project, &snap.header().tag)
        .unwrap();
    let pins = &back.header().dep_pins;
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0].name.as_str(), "cowboy");
    assert_eq!(pins[1].name.as_str(), "ra");
}

#[test]
fn store_list_tags_includes_snapshot_carrying_new_fields() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let snap = snapshot_with_new_fields();
    let project = snap.header().project.clone();
    let tag = snap.header().tag.clone();
    store.write(&snap).unwrap();
    let tags = store.list_tags(&project).unwrap();
    assert!(tags.contains(&tag));
}
