// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
    TypeName,
};
use backhopper_core::model::snapshot::{
    FORMAT_VERSION, FunArity, HrlFile, Module, RecordDecl, RecordField, Snapshot, SnapshotHeader,
    SpecSig, TypeDecl, Visibility,
};
use backhopper_core::snapshot::{format, parser};

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v3.1.6").unwrap(),
        branch: Some("main".into()),
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into(), "include".into()],
        generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
        generated_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
    }
}

fn ra_module() -> Module {
    let mut m = Module::new(ModuleName::new("ra").unwrap());
    m.behaviours.push(ModuleName::new("gen_statem").unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
    });
    m.exports.push(FunArity {
        name: FunctionName::new("process_command").unwrap(),
        arity: Arity::new(2),
    });
    m.exports.push(FunArity {
        name: FunctionName::new("process_command").unwrap(),
        arity: Arity::new(3),
    });
    m.specs.push(SpecSig {
        name: FunctionName::new("process_command").unwrap(),
        arity: Arity::new(2),
        signature: "process_command(A, B) -> ok".into(),
    });
    m
}

fn ra_header() -> HrlFile {
    let mut h = HrlFile::new("include/ra.hrl");
    h.types.push(TypeDecl {
        name: TypeName::new("ra_index").unwrap(),
        arity: Arity::new(0),
        rhs: "non_neg_integer()".into(),
    });
    h.records.push(RecordDecl {
        name: RecordName::new("cfg").unwrap(),
        fields: vec![
            RecordField {
                name: FieldName::new("id").unwrap(),
                type_repr: Some("ra_server_id()".into()),
            },
            RecordField {
                name: FieldName::new("uid").unwrap(),
                type_repr: Some("ra_uid()".into()),
            },
        ],
    });
    h
}

#[test]
fn round_trip_writes_then_parses() {
    let snap =
        Snapshot::from_extracted(header(), vec![ra_module()], vec![ra_header()]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn header_block_starts_canonical_text() {
    let snap = Snapshot::from_extracted(header(), vec![ra_module()], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(text.starts_with("# backhopper snapshot\n"));
    assert!(text.contains(&format!("# format-version: {}\n", FORMAT_VERSION)));
    assert!(text.contains("# project: ra\n"));
}

#[test]
fn parser_rejects_unknown_header_key() {
    let bad = "# backhopper snapshot
# format-version: 1
# project: ra
# tag: v3.1.6
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: backhopper 0.1.0
# generated-at: 2023-11-14T22:13:20Z
# nonsense: yes
";
    let r = parser::parse(bad);
    assert!(r.is_err());
}

#[test]
fn parser_rejects_wrong_format_version() {
    let bad = "# backhopper snapshot
# format-version: 99
# project: ra
# tag: v3.1.6
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: backhopper 0.1.0
# generated-at: 2023-11-14T22:13:20Z
";
    let r = parser::parse(bad);
    assert!(r.is_err());
}

#[test]
fn parser_rejects_non_canonical_export_order() {
    let bad = "# backhopper snapshot
# format-version: 1
# project: ra
# tag: v3.1.6
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: backhopper 0.1.0
# generated-at: 2023-11-14T22:13:20Z

module ra
  export z/1
  export a/1
";
    let r = parser::parse(bad);
    assert!(r.is_err());
}

#[test]
fn module_visibility_round_trips() {
    let mut m = Module::new(ModuleName::new("ra_server").unwrap());
    m.visibility = Visibility::Hidden;
    m.exports.push(FunArity {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
    });
    let snap = Snapshot::from_extracted(header(), vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(text.contains("  visibility hidden\n"));
    let back = parser::parse(&text).unwrap();
    assert_eq!(back.modules()[0].visibility, Visibility::Hidden);
}
