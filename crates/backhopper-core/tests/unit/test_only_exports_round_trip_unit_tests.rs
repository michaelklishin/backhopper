// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::{
    IfdefGuardKind, IfdefMacro, Module, Snapshot, TestExportVariant, TestOnlyExport, VariantCBlock,
    state,
};
use backhopper_core::snapshot::{format, parser};
use backhopper_test_support::snapshot_header;

fn module_with_extensions() -> Module {
    let mut m = Module::new(ModuleName::new("rabbit_khepri").unwrap());
    m.test_only_exports.push(TestOnlyExport {
        function: FunctionName::new("expand_mnesia_migrations").unwrap(),
        arity: Arity::new(1),
        export_line: 184,
        body_line: None,
        variant: TestExportVariant::A,
    });
    m.test_only_exports.push(TestOnlyExport {
        function: FunctionName::new("force_metadata_store").unwrap(),
        arity: Arity::new(1),
        export_line: 184,
        body_line: Some(2223),
        variant: TestExportVariant::B,
    });
    m.ifdef_macros.push(IfdefMacro {
        name: "FORCED_MDS_KEY".to_owned(),
        line: 2220,
        guard_kind: IfdefGuardKind::Test,
    });
    m.variant_c_blocks.push(VariantCBlock {
        guard: "TEST".to_owned(),
        start_line: 2118,
        else_line: Some(2128),
        end_line: 2137,
    });
    m
}

#[test]
fn module_extensions_round_trip_through_text_format() {
    let original = module_with_extensions();
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        snapshot_header("khepri", "v1.0.0"),
        vec![original.clone()],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    let reparsed = parser::parse(&text).expect("parse back");
    let parsed_module = reparsed
        .module_named(&ModuleName::new("rabbit_khepri").unwrap())
        .expect("module present");
    assert_eq!(parsed_module.test_only_exports, original.test_only_exports);
    assert_eq!(parsed_module.ifdef_macros, original.ifdef_macros);
    assert_eq!(parsed_module.variant_c_blocks, original.variant_c_blocks);
}

#[test]
fn variant_a_omits_body_line_in_wire_format() {
    let mut m = Module::new(ModuleName::new("ra_lib").unwrap());
    m.test_only_exports.push(TestOnlyExport {
        function: FunctionName::new("id").unwrap(),
        arity: Arity::new(1),
        export_line: 10,
        body_line: None,
        variant: TestExportVariant::A,
    });
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        snapshot_header("khepri", "v1.0.0"),
        vec![m],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    assert!(
        text.contains("test_only_export a id/1 export_line=10"),
        "text was:\n{text}"
    );
    assert!(
        !text.contains("body_line="),
        "Variant A must not emit body_line in text was:\n{text}"
    );
}

#[test]
fn variant_b_requires_body_line_in_wire_format() {
    let mut m = Module::new(ModuleName::new("ra_machine").unwrap());
    m.test_only_exports.push(TestOnlyExport {
        function: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
        export_line: 37,
        body_line: Some(87),
        variant: TestExportVariant::B,
    });
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        snapshot_header("khepri", "v1.0.0"),
        vec![m],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    assert!(
        text.contains("test_only_export b init/1 export_line=37 body_line=87"),
        "text was:\n{text}"
    );
}

#[test]
fn ifdef_macro_guard_kinds_round_trip() {
    let mut m = Module::new(ModuleName::new("osiris_log").unwrap());
    m.ifdef_macros.push(IfdefMacro {
        name: "DEBUG".to_owned(),
        line: 10,
        guard_kind: IfdefGuardKind::Test,
    });
    m.ifdef_macros.push(IfdefMacro {
        name: "MAX_CHUNK_SIZE".to_owned(),
        line: 20,
        guard_kind: IfdefGuardKind::NotTest,
    });
    m.ifdef_macros.push(IfdefMacro {
        name: "TRACING".to_owned(),
        line: 30,
        guard_kind: IfdefGuardKind::Other,
    });
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        snapshot_header("khepri", "v1.0.0"),
        vec![m.clone()],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    let reparsed = parser::parse(&text).expect("parse");
    let parsed = reparsed
        .module_named(&ModuleName::new("osiris_log").unwrap())
        .unwrap();
    assert_eq!(parsed.ifdef_macros, m.ifdef_macros);
}

#[test]
fn variant_c_block_without_else_round_trips() {
    let mut m = Module::new(ModuleName::new("khepri_machine").unwrap());
    m.variant_c_blocks.push(VariantCBlock {
        guard: "TEST".to_owned(),
        start_line: 50,
        else_line: None,
        end_line: 100,
    });
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        snapshot_header("khepri", "v1.0.0"),
        vec![m.clone()],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    let reparsed = parser::parse(&text).expect("parse");
    let parsed = reparsed
        .module_named(&ModuleName::new("khepri_machine").unwrap())
        .unwrap();
    assert_eq!(parsed.variant_c_blocks, m.variant_c_blocks);
}

const HEADER: &str = "# backhopper snapshot
# format-version: 3
# project: khepri
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 2026-05-29T00:00:00Z

";

fn snapshot_with_module_body(body: &str) -> String {
    format!("{HEADER}module rabbit_fifo\n{body}")
}

#[test]
fn parser_rejects_test_only_export_with_unknown_variant() {
    let text = snapshot_with_module_body("  test_only_export c purge/1 export_line=10\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn parser_rejects_test_only_export_missing_export_line() {
    let text = snapshot_with_module_body("  test_only_export a overview/1 body_line=20\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn parser_rejects_variant_b_without_body_line() {
    let text = snapshot_with_module_body("  test_only_export b init/1 export_line=10\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn parser_rejects_ifdef_macro_with_unknown_guard() {
    let text = snapshot_with_module_body("  ifdef_macro DEBUG guard=cosmic line=10\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn parser_rejects_variant_c_block_missing_end_line() {
    let text = snapshot_with_module_body("  variant_c_block guard=TEST start_line=10\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn empty_extensions_emit_no_lines() {
    let m = Module::new(ModuleName::new("ra_directory").unwrap());
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        snapshot_header("khepri", "v1.0.0"),
        vec![m],
        vec![],
    )
    .into_canonical();
    let text = format::to_string(&snap).expect("write");
    // None of the new sections should appear when their fields are empty.
    assert!(!text.contains("test_only_export"), "text was:\n{text}");
    assert!(!text.contains("ifdef_macro"), "text was:\n{text}");
    assert!(!text.contains("variant_c_block"), "text was:\n{text}");
}
