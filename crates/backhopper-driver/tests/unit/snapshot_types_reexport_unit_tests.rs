// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use time::OffsetDateTime;

use backhopper_driver::types::{
    Arity, CommitSha, DependencyName, DependencyVersion, FORMAT_VERSION, FunctionName, MacroName,
    Module, ModuleName, ProjectName, Provenance, SUPPORTED_FORMAT_VERSIONS, Snapshot,
    SnapshotHeader, TagName, VendoredDep, VendoredDepSource, VersionedMachineVersion,
    WireConstantBinding, WireValue, snapshot_state,
};

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("rabbitmq-server").unwrap(),
        tag: TagName::new("v4.0.5").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["deps".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper-driver test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: "3".into(),
        dep_pins: vec![VendoredDep {
            name: DependencyName::new("ra").unwrap(),
            version: DependencyVersion::new("3.1.6").unwrap(),
            source: VendoredDepSource::Hex,
        }],
    }
}

fn rabbit_fifo() -> Module {
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

#[test]
fn driver_re_exports_format_version_constants() {
    assert_eq!(FORMAT_VERSION, 4);
    assert!(SUPPORTED_FORMAT_VERSIONS.contains(&1));
    assert!(SUPPORTED_FORMAT_VERSIONS.contains(&4));
}

#[test]
fn snapshot_envelope_round_trips_through_driver_re_exports() {
    let snap: Snapshot<snapshot_state::Canonical> =
        Snapshot::from_extracted(header(), vec![rabbit_fifo()], vec![]).into_canonical();
    let encoded = serde_json::to_value(&snap).unwrap();
    let back: Snapshot<snapshot_state::Canonical> = serde_json::from_value(encoded).unwrap();
    assert_eq!(snap, back);
    let m = back
        .module_named(&ModuleName::new("rabbit_fifo").unwrap())
        .unwrap();
    let v = m.versioned_machine_version.as_ref().unwrap();
    assert_eq!(v.value, Some(7));
    assert_eq!(v.arity, Arity::new(0));
    assert!(matches!(&v.provenance, Provenance::MacroBody { .. }));
    assert_eq!(m.wire_constants.len(), 1);
}

#[test]
fn driver_consumer_can_match_on_wire_value_variants() {
    let bindings = [
        WireValue::U64(42),
        WireValue::Bytes(b"RASG".to_vec()),
        WireValue::Opaque("{tuple, 1}".into()),
    ];
    let kinds: Vec<&str> = bindings
        .iter()
        .map(|v| match v {
            WireValue::U64(_) => "u64",
            WireValue::Bytes(_) => "bytes",
            WireValue::Opaque(_) => "opaque",
        })
        .collect();
    assert_eq!(kinds, vec!["u64", "bytes", "opaque"]);
}

#[test]
fn driver_consumer_can_match_on_provenance_variants() {
    let cases = [
        Provenance::Literal,
        Provenance::MacroBody {
            macro_name: MacroName::new("MV").unwrap(),
            defined_in: None,
        },
    ];
    let kinds: Vec<&str> = cases
        .iter()
        .map(|p| match p {
            Provenance::Literal => "literal",
            Provenance::MacroBody { .. } => "macro_body",
        })
        .collect();
    assert_eq!(kinds, vec!["literal", "macro_body"]);
}

#[test]
fn driver_consumer_can_match_on_vendored_dep_source() {
    let kinds: Vec<&str> = [
        VendoredDepSource::Hex,
        VendoredDepSource::Git,
        VendoredDepSource::GitRmq,
    ]
    .iter()
    .map(|s| match s {
        VendoredDepSource::Hex => "hex",
        VendoredDepSource::Git => "git",
        VendoredDepSource::GitRmq => "git_rmq",
    })
    .collect();
    assert_eq!(kinds, vec!["hex", "git", "git_rmq"]);
}

#[test]
fn snapshot_dep_pins_round_trip_through_driver_re_exported_serde() {
    let snap = Snapshot::from_extracted(header(), vec![], vec![]).into_canonical();
    let encoded = serde_json::to_value(&snap).unwrap();
    let back: Snapshot<snapshot_state::Canonical> = serde_json::from_value(encoded).unwrap();
    assert_eq!(back.header().dep_pins.len(), 1);
    let pin = &back.header().dep_pins[0];
    assert_eq!(pin.name.as_str(), "ra");
    assert_eq!(pin.version.as_str(), "3.1.6");
    assert!(matches!(pin.source, VendoredDepSource::Hex));
}
