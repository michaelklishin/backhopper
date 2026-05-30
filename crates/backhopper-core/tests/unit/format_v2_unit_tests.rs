// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::model::names::{
    ApplicationName, CommitSha, FieldName, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::snapshot::{
    FORMAT_VERSION, Module, RecordDecl, RecordField, SUPPORTED_FORMAT_VERSIONS, Snapshot,
    SnapshotHeader, state,
};
use backhopper_core::snapshot::{format, parser};
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

#[test]
fn format_version_constant_is_three() {
    assert_eq!(FORMAT_VERSION, 3);
}

#[test]
fn supported_versions_include_v1_v2_and_v3() {
    assert!(SUPPORTED_FORMAT_VERSIONS.contains(&1));
    assert!(SUPPORTED_FORMAT_VERSIONS.contains(&2));
    assert!(SUPPORTED_FORMAT_VERSIONS.contains(&3));
}

fn v1_text(body: &str) -> String {
    format!(
        "# backhopper snapshot\n# format-version: 1\n# project: p\n# tag: v1.0.0\n# commit: 0000000000000000000000000000000000000000\n# scanned-paths: src\n# generated-by: test\n# generated-at: 1970-01-01T00:00:00Z\n\n{body}",
    )
}

fn v2_text(body: &str) -> String {
    format!(
        "# backhopper snapshot\n# format-version: {FORMAT_VERSION}\n# project: p\n# tag: v1.0.0\n# commit: 0000000000000000000000000000000000000000\n# scanned-paths: src\n# generated-by: test\n# generated-at: 1970-01-01T00:00:00Z\n\n{body}",
    )
}

#[test]
fn parser_accepts_v1_format() {
    let text = v1_text("module alpha\n  export hello/0\n");
    let parsed = parser::parse(&text).expect("v1 snapshot parses");
    assert_eq!(parsed.modules().len(), 1);
    assert_eq!(parsed.modules()[0].name.as_str(), "alpha");
}

#[test]
fn parser_accepts_v2_format() {
    let text = v2_text("module alpha\n  path src/alpha.erl\n  app rabbit\n  export hello/0\n");
    let parsed = parser::parse(&text).expect("v2 snapshot parses");
    let alpha = parsed
        .module_named(&ModuleName::new("alpha").unwrap())
        .unwrap();
    assert_eq!(alpha.path.as_deref(), Some("src/alpha.erl"));
    assert_eq!(alpha.app.as_ref().map(|a| a.as_str()), Some("rabbit"));
}

#[test]
fn parser_rejects_unknown_format_version() {
    let text = "# backhopper snapshot\n# format-version: 99\n# project: p\n# tag: v1.0.0\n# commit: 0000000000000000000000000000000000000000\n# scanned-paths: src\n# generated-by: test\n# generated-at: 1970-01-01T00:00:00Z\n";
    assert!(parser::parse(text).is_err());
}

#[test]
fn writer_emits_current_format_header() {
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        header(),
        vec![Module::new(ModuleName::new("alpha").unwrap())],
        Vec::new(),
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    let expected = format!("# format-version: {FORMAT_VERSION}");
    assert!(text.contains(&expected), "text was:\n{text}");
}

#[test]
fn module_path_and_app_round_trip_through_format() {
    let mut m = Module::new(ModuleName::new("alpha").unwrap());
    m.path = Some("deps/rabbit/src/alpha.erl".into());
    m.app = Some(ApplicationName::new("rabbit").unwrap());
    let snap =
        Snapshot::<state::Unsorted>::from_extracted(header(), vec![m], Vec::new()).into_canonical();
    let text = format::to_string(&snap).expect("write");
    let reparsed = parser::parse(&text).expect("re-parse");
    let alpha = reparsed
        .module_named(&ModuleName::new("alpha").unwrap())
        .unwrap();
    assert_eq!(alpha.path.as_deref(), Some("deps/rabbit/src/alpha.erl"));
    assert_eq!(alpha.app.as_ref().map(|a| a.as_str()), Some("rabbit"));
}

#[test]
fn module_records_round_trip_through_format() {
    let mut m = Module::new(ModuleName::new("alpha").unwrap());
    m.records.push(RecordDecl {
        name: RecordName::new("state").unwrap(),
        fields: vec![
            RecordField {
                name: FieldName::new("a").unwrap(),
                type_repr: None,
            },
            RecordField {
                name: FieldName::new("b").unwrap(),
                type_repr: Some("integer()".into()),
            },
        ],
    });
    let snap =
        Snapshot::<state::Unsorted>::from_extracted(header(), vec![m], Vec::new()).into_canonical();
    let text = format::to_string(&snap).expect("write");
    let reparsed = parser::parse(&text).expect("re-parse");
    let alpha = reparsed
        .module_named(&ModuleName::new("alpha").unwrap())
        .unwrap();
    assert_eq!(alpha.records.len(), 1);
    assert_eq!(alpha.records[0].name.as_str(), "state");
    assert_eq!(alpha.records[0].fields.len(), 2);
    assert_eq!(
        alpha.records[0].fields[1].type_repr.as_deref(),
        Some("integer()")
    );
}
