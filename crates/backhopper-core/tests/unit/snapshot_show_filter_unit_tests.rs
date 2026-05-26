// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SnapshotHeader};
use backhopper_core::snapshot::format;

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("p").unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
    }
}

fn module(name: &str, exports: &[(&str, u8)]) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    for (n, a) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(*n).unwrap(),
            arity: Arity::new(*a),
        });
    }
    m
}

#[test]
fn write_module_filtered_returns_true_when_module_present() {
    let snap = Snapshot::from_extracted(
        header(),
        vec![module("alpha", &[("f", 1)]), module("beta", &[("g", 2)])],
        vec![],
    )
    .into_canonical();
    let mut buf = Vec::new();
    let found = format::write_module_filtered(&snap, &ModuleName::new("beta").unwrap(), &mut buf)
        .expect("write");
    assert!(found);
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("module beta"), "{text}");
    assert!(text.contains("export g/2"), "{text}");
    assert!(!text.contains("module alpha"), "{text}");
    assert!(!text.contains("export f/1"), "{text}");
}

#[test]
fn write_module_filtered_returns_false_when_module_absent() {
    let snap = Snapshot::from_extracted(header(), vec![module("alpha", &[("f", 1)])], vec![])
        .into_canonical();
    let mut buf = Vec::new();
    let found = format::write_module_filtered(&snap, &ModuleName::new("nope").unwrap(), &mut buf)
        .expect("write");
    assert!(!found);
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("# backhopper snapshot"), "{text}");
    assert!(!text.contains("module"), "{text}");
}

#[test]
fn write_module_filtered_output_re_parses_as_canonical() {
    use backhopper_core::snapshot::parser;
    let snap = Snapshot::from_extracted(
        header(),
        vec![module("alpha", &[("f", 1)]), module("beta", &[("g", 2)])],
        vec![],
    )
    .into_canonical();
    let mut buf = Vec::new();
    format::write_module_filtered(&snap, &ModuleName::new("beta").unwrap(), &mut buf)
        .expect("write");
    let text = String::from_utf8(buf).unwrap();
    let back = parser::parse(&text).expect("re-parse filtered output");
    assert_eq!(back.modules().len(), 1);
    assert_eq!(back.modules()[0].name.as_str(), "beta");
}

#[test]
fn write_module_filtered_writes_no_trailing_modules_section() {
    let snap = Snapshot::from_extracted(
        header(),
        vec![module("alpha", &[]), module("beta", &[])],
        vec![],
    )
    .into_canonical();
    let mut buf = Vec::new();
    format::write_module_filtered(&snap, &ModuleName::new("beta").unwrap(), &mut buf)
        .expect("write");
    let text = String::from_utf8(buf).unwrap();
    assert_eq!(text.matches("\nmodule ").count(), 1);
}

#[test]
fn write_module_filtered_preserves_header() {
    let snap =
        Snapshot::from_extracted(header(), vec![module("alpha", &[])], vec![]).into_canonical();
    let mut buf = Vec::new();
    format::write_module_filtered(&snap, &ModuleName::new("alpha").unwrap(), &mut buf)
        .expect("write");
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("# project: p"));
    assert!(text.contains("# tag: v1.0.0"));
    assert!(text.contains("# format-version: 1"));
}
