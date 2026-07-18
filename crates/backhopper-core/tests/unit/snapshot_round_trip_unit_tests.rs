// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    ApplicationName, Arity, CommitSha, DependencyName, DependencyVersion, FieldName, FunctionName,
    MacroName, ModuleName, ProjectName, RecordName, TagName, TypeName,
};
use backhopper_core::model::snapshot::{
    ArityMatch, CallbackSig, Deprecation, DeprecationReplacement, FORMAT_VERSION, FunArity,
    HrlFile, IfdefGuardKind, IfdefMacro, Module, Provenance, RecordDecl, RecordField, Snapshot,
    SnapshotHeader, SpecSig, TestExportVariant, TestOnlyExport, TypeArity, TypeDecl, VariantCBlock,
    VendoredDep, VendoredDepSource, VersionedMachineVersion, Visibility, WireConstantBinding,
    WireValue,
};
use backhopper_core::snapshot::{format, parser};

fn header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v3.1.6").unwrap(),
        branch: Some("main".into()),
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into(), "include".into()],
        apps_scanned: Vec::new(),
        generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
        generated_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
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
    assert!(text.contains(&format!("# format-version: {FORMAT_VERSION}\n")));
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

// A deprecation reason with quotes, a backslash, and whitespace runs must round-trip unchanged.
#[test]
fn deprecation_reason_with_special_chars_round_trips() {
    let mut m = Module::new(ModuleName::new("ra_server").unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
    });
    let reason = r#"use "ra:new"  instead\not this"#.to_owned();
    m.deprecations.push(Deprecation {
        function: Some(FunctionName::new("init").unwrap()),
        arity_match: ArityMatch::Exact {
            arity: Arity::new(1),
        },
        since: None,
        replacement: None,
        reason: Some(reason.clone()),
        module_wide: false,
    });
    let snap = Snapshot::from_extracted(header(), vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(
        back.modules()[0].deprecations[0].reason.as_deref(),
        Some(reason.as_str())
    );
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

// A ra_machine-shaped module exercising every advanced entry kind must
// round-trip unchanged, pinning the parser and writer as inverses.
#[test]
fn round_trip_preserves_all_advanced_entry_kinds() {
    let mut h = header();
    h.apps_scanned = vec![
        ApplicationName::new("ra").unwrap(),
        ApplicationName::new("kernel").unwrap(),
    ];
    h.dep_pins = vec![
        VendoredDep {
            name: DependencyName::new("aten").unwrap(),
            version: DependencyVersion::new("0.9.1").unwrap(),
            source: VendoredDepSource::Hex,
        },
        VendoredDep {
            name: DependencyName::new("seshat").unwrap(),
            version: DependencyVersion::new("0.6.1").unwrap(),
            source: VendoredDepSource::GitRmq,
        },
    ];

    let mut m = Module::new(ModuleName::new("ra_machine").unwrap());
    m.behaviours.push(ModuleName::new("gen_statem").unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new("apply").unwrap(),
        arity: Arity::new(3),
    });
    m.exports.push(FunArity {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
    });
    m.export_types.push(TypeArity {
        name: TypeName::new("state").unwrap(),
        arity: Arity::new(0),
    });
    m.callbacks.push(CallbackSig {
        name: FunctionName::new("apply").unwrap(),
        arity: Arity::new(3),
        signature: "apply(map(), term(), State) -> {State, term()}".into(),
    });
    m.callbacks.push(CallbackSig {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
        signature: "init(map()) -> state()".into(),
    });
    m.optional_callbacks.push(FunArity {
        name: FunctionName::new("state_enter").unwrap(),
        arity: Arity::new(2),
    });
    m.specs.push(SpecSig {
        name: FunctionName::new("init").unwrap(),
        arity: Arity::new(1),
        signature: "init(map()) -> state()".into(),
    });
    m.types.push(TypeDecl {
        name: TypeName::new("command").unwrap(),
        arity: Arity::new(0),
        rhs: "term()".into(),
    });
    m.opaques.push(TypeArity {
        name: TypeName::new("state").unwrap(),
        arity: Arity::new(0),
    });
    m.test_only_exports.push(TestOnlyExport {
        function: FunctionName::new("test_init").unwrap(),
        arity: Arity::new(0),
        export_line: 42,
        body_line: None,
        variant: TestExportVariant::A,
    });
    m.test_only_exports.push(TestOnlyExport {
        function: FunctionName::new("test_apply").unwrap(),
        arity: Arity::new(1),
        export_line: 50,
        body_line: Some(120),
        variant: TestExportVariant::B,
    });
    m.ifdef_macros.push(IfdefMacro {
        name: "TEST".into(),
        line: 10,
        guard_kind: IfdefGuardKind::Test,
    });
    m.ifdef_macros.push(IfdefMacro {
        name: "DEBUG".into(),
        line: 12,
        guard_kind: IfdefGuardKind::Other,
    });
    m.variant_c_blocks.push(VariantCBlock {
        guard: "TEST".into(),
        start_line: 60,
        end_line: 80,
        else_line: Some(70),
    });
    m.versioned_machine_version = Some(VersionedMachineVersion {
        function: FunctionName::new("version").unwrap(),
        arity: Arity::new(0),
        value: Some(0),
        provenance: Provenance::MacroBody {
            macro_name: MacroName::new("DEFAULT_VERSION").unwrap(),
            defined_in: Some("src/ra_machine.erl".into()),
        },
    });
    m.wire_constants.push(WireConstantBinding {
        macro_name: MacroName::new("RA_PROTO_VERSION").unwrap(),
        value: WireValue::U64(2),
        defined_in: Some("src/ra.hrl".into()),
    });
    m.wire_constants.push(WireConstantBinding {
        macro_name: MacroName::new("MAGIC").unwrap(),
        value: WireValue::Bytes(b"RASG".to_vec()),
        defined_in: None,
    });

    let mut hrl = HrlFile::new("include/ra.hrl");
    hrl.types.push(TypeDecl {
        name: TypeName::new("ra_index").unwrap(),
        arity: Arity::new(0),
        rhs: "non_neg_integer()".into(),
    });
    hrl.opaques.push(TypeArity {
        name: TypeName::new("ra_idxterm").unwrap(),
        arity: Arity::new(0),
    });

    let snap = Snapshot::from_extracted(h, vec![m], vec![hrl]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}

// Variants the comprehensive test skipped: literal version provenance, opaque wire
// value, module-wide deprecation, and a function deprecation with since, use, and reason.
#[test]
fn round_trip_preserves_literal_provenance_opaque_wire_and_deprecations() {
    let mut m = Module::new(ModuleName::new("ra_log").unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new("append").unwrap(),
        arity: Arity::new(2),
    });
    // Module-wide deprecations carry no since or reason: the round-trip leaves those None.
    m.deprecations.push(Deprecation {
        function: None,
        arity_match: ArityMatch::Any,
        since: None,
        replacement: None,
        reason: None,
        module_wide: true,
    });
    m.deprecations.push(Deprecation {
        function: Some(FunctionName::new("old_append").unwrap()),
        arity_match: ArityMatch::Exact {
            arity: Arity::new(2),
        },
        since: Some(TagName::new("v2.0.0").unwrap()),
        replacement: Some(DeprecationReplacement {
            function: FunctionName::new("append").unwrap(),
            arity: Arity::new(2),
        }),
        reason: Some("renamed for clarity".into()),
        module_wide: false,
    });
    m.versioned_machine_version = Some(VersionedMachineVersion {
        function: FunctionName::new("version").unwrap(),
        arity: Arity::new(0),
        value: Some(1),
        provenance: Provenance::Literal,
    });
    m.wire_constants.push(WireConstantBinding {
        macro_name: MacroName::new("SEGMENT_LAYOUT").unwrap(),
        value: WireValue::Opaque("#{version => 2}".into()),
        defined_in: Some("src/ra_log.erl".into()),
    });

    let snap = Snapshot::from_extracted(header(), vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}

// A valid header block the error-path tests append a malformed body to.
fn valid_header_text() -> &'static str {
    "# backhopper snapshot
# format-version: 1
# project: ra
# tag: v3.1.6
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: backhopper 0.1.0
# generated-at: 2023-11-14T22:13:20Z
"
}

#[test]
fn parser_rejects_non_canonical_module_order() {
    let bad = format!(
        "{}\nmodule ra_server\n  export init/1\nmodule ra_log\n  export append/2\n",
        valid_header_text()
    );
    assert!(parser::parse(&bad).is_err());
}

#[test]
fn parser_rejects_non_canonical_header_order() {
    let bad = format!(
        "{}\nheader include/z.hrl\n  type t/0 :: atom()\nheader include/a.hrl\n  type t/0 :: atom()\n",
        valid_header_text()
    );
    assert!(parser::parse(&bad).is_err());
}

#[test]
fn parser_rejects_non_canonical_behaviour_order() {
    let bad = format!(
        "{}\nmodule ra_server\n  behaviour supervisor\n  behaviour gen_statem\n  export init/1\n",
        valid_header_text()
    );
    assert!(parser::parse(&bad).is_err());
}

#[test]
fn parser_rejects_unknown_visibility_keyword() {
    let bad = format!(
        "{}\nmodule ra_server\n  visibility invisible\n  export init/1\n",
        valid_header_text()
    );
    assert!(parser::parse(&bad).is_err());
}

#[test]
fn parser_rejects_invalid_generated_at_timestamp() {
    let bad = "# backhopper snapshot
# format-version: 1
# project: ra
# tag: v3.1.6
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: backhopper 0.1.0
# generated-at: not-a-real-timestamp
";
    assert!(parser::parse(bad).is_err());
}
