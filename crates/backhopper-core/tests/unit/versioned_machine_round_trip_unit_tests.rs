// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, DependencyName, DependencyVersion, FunctionName, MacroName, ModuleName,
    ProjectName, TagName,
};
use backhopper_core::model::snapshot::{
    Module, Provenance, Snapshot, SnapshotHeader, VendoredDep, VendoredDepSource,
    VersionedMachineVersion, WireConstantBinding, WireValue,
};
use backhopper_core::snapshot::{format, parser};

fn empty_header() -> SnapshotHeader {
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
        dep_pins: Vec::new(),
    }
}

fn rabbit_fifo_module(value: u64, provenance: Provenance) -> Module {
    let mut m = Module::new(ModuleName::new("rabbit_fifo").unwrap());
    m.versioned_machine_version = Some(VersionedMachineVersion {
        function: FunctionName::from_str("version").unwrap(),
        arity: Arity::new(0),
        value: Some(value),
        provenance,
    });
    m
}

fn ra_module() -> Module {
    let mut m = Module::new(ModuleName::new("ra").unwrap());
    m.wire_constants = vec![
        WireConstantBinding {
            macro_name: MacroName::new("RA_PROTO_VERSION").unwrap(),
            value: WireValue::U64(1),
            defined_in: None,
        },
        WireConstantBinding {
            macro_name: MacroName::new("MAGIC").unwrap(),
            value: WireValue::Bytes(b"RASG".to_vec()),
            defined_in: Some("src/ra.erl".into()),
        },
    ];
    m
}

fn dep_ra() -> VendoredDep {
    VendoredDep {
        name: DependencyName::new("ra").unwrap(),
        version: DependencyVersion::new("3.1.6").unwrap(),
        source: VendoredDepSource::Hex,
    }
}

#[test]
fn snapshot_with_versioned_machine_literal_round_trips_through_text_format() {
    let header = empty_header();
    let m = rabbit_fifo_module(7, Provenance::Literal);
    let snap = Snapshot::from_extracted(header, vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert!(text.contains("versioned_machine version/0 = 7 literal"));
}

#[test]
fn snapshot_with_versioned_machine_macro_body_round_trips_with_defined_in() {
    let header = empty_header();
    let m = rabbit_fifo_module(
        8,
        Provenance::MacroBody {
            macro_name: MacroName::new("MACHINE_VERSION").unwrap(),
            defined_in: Some("deps/rabbit/include/rabbit_fifo.hrl".into()),
        },
    );
    let snap = Snapshot::from_extracted(header, vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert!(text.contains(
        "versioned_machine version/0 = 8 macro_body MACHINE_VERSION at deps/rabbit/include/rabbit_fifo.hrl"
    ));
}

#[test]
fn snapshot_with_versioned_machine_macro_body_without_defined_in_round_trips() {
    let header = empty_header();
    let m = rabbit_fifo_module(
        9,
        Provenance::MacroBody {
            macro_name: MacroName::new("MV").unwrap(),
            defined_in: None,
        },
    );
    let snap = Snapshot::from_extracted(header, vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert!(text.contains("versioned_machine version/0 = 9 macro_body MV\n"));
}

#[test]
fn snapshot_with_unresolved_versioned_machine_writes_dash_marker() {
    let header = empty_header();
    let mut m = Module::new(ModuleName::new("rabbit_fifo").unwrap());
    m.versioned_machine_version = Some(VersionedMachineVersion {
        function: FunctionName::from_str("version").unwrap(),
        arity: Arity::new(0),
        value: None,
        provenance: Provenance::Literal,
    });
    let snap = Snapshot::from_extracted(header, vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert!(text.contains("versioned_machine version/0 = - literal"));
}

#[test]
fn snapshot_with_wire_constants_round_trips_in_macro_name_order() {
    let header = empty_header();
    let snap = Snapshot::from_extracted(header, vec![ra_module()], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    let magic_pos = text.find("wire_constant MAGIC").unwrap();
    let proto_pos = text.find("wire_constant RA_PROTO_VERSION").unwrap();
    assert!(
        magic_pos < proto_pos,
        "MAGIC must sort before RA_PROTO_VERSION"
    );
}

#[test]
fn snapshot_with_dep_pins_round_trips_through_header_lines() {
    let mut header = empty_header();
    header.dep_pins = vec![
        dep_ra(),
        VendoredDep {
            name: DependencyName::new("cowboy").unwrap(),
            version: DependencyVersion::new("2.13.0").unwrap(),
            source: VendoredDepSource::GitRmq,
        },
    ];
    let snap = Snapshot::from_extracted(header, vec![], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    let cowboy = text.find("# dep-pin: cowboy = git_rmq 2.13.0").unwrap();
    let ra = text.find("# dep-pin: ra = hex 3.1.6").unwrap();
    assert!(cowboy < ra, "dep-pins must sort alphabetically by name");
}

#[test]
fn snapshot_with_all_three_surfaces_round_trips_together() {
    let mut header = empty_header();
    header.dep_pins = vec![dep_ra()];
    let snap = Snapshot::from_extracted(
        header,
        vec![ra_module(), rabbit_fifo_module(7, Provenance::Literal)],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn canonical_writer_emits_format_version_four() {
    let header = empty_header();
    let snap = Snapshot::from_extracted(header, vec![], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(text.contains("# format-version: 4"));
}

#[test]
fn parser_accepts_v3_snapshot_without_new_fields() {
    let v3 = "\
# backhopper snapshot
# format-version: 3
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module foo
  export f/0
";
    let snap = parser::parse(v3).expect("v3 snapshot parses with v4 binary");
    let m = snap.module_named(&ModuleName::new("foo").unwrap()).unwrap();
    assert_eq!(m.versioned_machine_version, None);
    assert!(m.wire_constants.is_empty());
    assert!(snap.header().dep_pins.is_empty());
}

#[test]
fn parser_rejects_duplicate_versioned_machine_entry() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module rabbit_fifo
  versioned_machine version/0 = 7 literal
  versioned_machine version/0 = 8 literal
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_wire_constants_out_of_canonical_order() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module ra
  wire_constant RA_PROTO_VERSION = u64 1
  wire_constant MAGIC = bytes RASG
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_unknown_dep_pin_source() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z
# dep-pin: ra = svn 3.1.6

";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_dep_pin_missing_equals() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z
# dep-pin: ra hex 3.1.6

";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_versioned_machine_missing_equals() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module rabbit_fifo
  versioned_machine version/0 7 literal
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_wire_constant_unknown_kind() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module ra
  wire_constant FOO = i64 -1
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_versioned_machine_after_wire_constant() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module rabbit_fifo
  wire_constant FOO = u64 1
  versioned_machine version/0 = 7 literal
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_accepts_macro_body_without_defined_in() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module rabbit_fifo
  versioned_machine version/0 = 7 macro_body MV
";
    let snap = parser::parse(text).expect("valid v4 snapshot parses");
    let m = snap
        .module_named(&ModuleName::new("rabbit_fifo").unwrap())
        .unwrap();
    let v = m.versioned_machine_version.as_ref().unwrap();
    assert_eq!(v.value, Some(7));
    assert!(matches!(
        &v.provenance,
        Provenance::MacroBody {
            defined_in: None,
            ..
        }
    ));
}

#[test]
fn parser_rejects_unknown_provenance_keyword() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module rabbit_fifo
  versioned_machine version/0 = 7 litral
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_wire_constant_u64_with_non_integer_value() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module ra
  wire_constant FOO = u64 abc
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_rejects_versioned_machine_with_negative_value() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module rabbit_fifo
  versioned_machine version/0 = -1 literal
";
    assert!(parser::parse(text).is_err());
}

#[test]
fn parser_round_trips_wire_constant_with_defined_in() {
    let text = "\
# backhopper snapshot
# format-version: 4
# project: p
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 1970-01-01T00:00:00Z

module ra
  wire_constant RA_PROTO_VERSION = u64 1 at src/ra.erl
";
    let snap = parser::parse(text).expect("valid v4 snapshot parses");
    let m = snap.module_named(&ModuleName::new("ra").unwrap()).unwrap();
    let wc = &m.wire_constants[0];
    assert_eq!(wc.defined_in.as_deref(), Some("src/ra.erl"));
    assert_eq!(wc.value, WireValue::U64(1));
}
