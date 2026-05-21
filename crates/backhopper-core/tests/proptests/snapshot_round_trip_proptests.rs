// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
    TypeName,
};
use backhopper_core::model::snapshot::{
    CallbackSig, FunArity, HrlFile, Module, RecordDecl, RecordField, Snapshot, SnapshotHeader,
    SpecSig, TypeArity, TypeDecl, Visibility,
};
use backhopper_core::snapshot::{format, parser};

fn arb_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}"
}

fn arb_module_name() -> impl Strategy<Value = ModuleName> {
    arb_atom().prop_map(|s| ModuleName::new(s).unwrap())
}
fn arb_function_name() -> impl Strategy<Value = FunctionName> {
    arb_atom().prop_map(|s| FunctionName::new(s).unwrap())
}
fn arb_type_name() -> impl Strategy<Value = TypeName> {
    arb_atom().prop_map(|s| TypeName::new(s).unwrap())
}
fn arb_record_name() -> impl Strategy<Value = RecordName> {
    arb_atom().prop_map(|s| RecordName::new(s).unwrap())
}
fn arb_field_name() -> impl Strategy<Value = FieldName> {
    arb_atom().prop_map(|s| FieldName::new(s).unwrap())
}
fn arb_arity() -> impl Strategy<Value = Arity> {
    (0u8..=10).prop_map(Arity::new)
}
fn arb_visibility() -> impl Strategy<Value = Visibility> {
    prop_oneof![
        Just(Visibility::Public),
        Just(Visibility::Hidden),
        Just(Visibility::TestOnly),
    ]
}
fn arb_fun_arity() -> impl Strategy<Value = FunArity> {
    (arb_function_name(), arb_arity()).prop_map(|(name, arity)| FunArity { name, arity })
}
fn arb_type_arity() -> impl Strategy<Value = TypeArity> {
    (arb_type_name(), arb_arity()).prop_map(|(name, arity)| TypeArity { name, arity })
}

fn arb_type_rhs() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z][a-z0-9_]*\\(\\)", 1..3).prop_map(|parts| parts.join(" | "))
}

fn arb_record_field_type() -> impl Strategy<Value = Option<String>> {
    let alternatives = prop::collection::vec("[a-z][a-z0-9_]*\\(\\)", 1..4);
    let multi_line = alternatives.prop_map(|parts| Some(parts.join(" |\n      ")));
    prop_oneof![multi_line, Just(None)]
}

fn arb_record_field() -> impl Strategy<Value = RecordField> {
    (arb_field_name(), arb_record_field_type())
        .prop_map(|(name, type_repr)| RecordField { name, type_repr })
}

fn arb_record_decl() -> impl Strategy<Value = RecordDecl> {
    (
        arb_record_name(),
        prop::collection::vec(arb_record_field(), 0..4),
    )
        .prop_map(|(name, fields)| RecordDecl { name, fields })
}

fn arb_callback_sig() -> impl Strategy<Value = CallbackSig> {
    (arb_function_name(), arb_arity(), arb_type_rhs()).prop_map(|(name, arity, signature)| {
        CallbackSig {
            name,
            arity,
            signature,
        }
    })
}

fn arb_spec_sig() -> impl Strategy<Value = SpecSig> {
    (arb_function_name(), arb_arity(), arb_type_rhs()).prop_map(|(name, arity, signature)| {
        SpecSig {
            name,
            arity,
            signature,
        }
    })
}

fn arb_type_decl() -> impl Strategy<Value = TypeDecl> {
    (arb_type_name(), arb_arity(), arb_type_rhs()).prop_map(|(name, arity, rhs)| TypeDecl {
        name,
        arity,
        rhs,
    })
}

fn arb_module() -> impl Strategy<Value = Module> {
    (
        arb_module_name(),
        arb_visibility(),
        prop::collection::vec(arb_module_name(), 0..3),
        prop::collection::vec(arb_fun_arity(), 0..4),
        prop::collection::vec(arb_type_arity(), 0..3),
        prop::collection::vec(arb_callback_sig(), 0..2),
        prop::collection::vec(arb_fun_arity(), 0..3),
        prop::collection::vec(arb_spec_sig(), 0..3),
        prop::collection::vec(arb_type_decl(), 0..3),
        prop::collection::vec(arb_type_arity(), 0..2),
    )
        .prop_map(
            |(
                name,
                visibility,
                behaviours,
                exports,
                export_types,
                callbacks,
                optional_callbacks,
                specs,
                types,
                opaques,
            )| {
                let mut m = Module::new(name);
                m.visibility = visibility;
                m.behaviours = behaviours;
                m.exports = exports;
                m.export_types = export_types;
                m.callbacks = callbacks;
                m.optional_callbacks = optional_callbacks;
                m.specs = specs;
                m.types = types;
                m.opaques = opaques;
                m
            },
        )
}

fn arb_hrl() -> impl Strategy<Value = HrlFile> {
    (
        "[a-z]{1,4}/[a-z]{1,4}\\.hrl",
        prop::collection::vec(arb_type_decl(), 0..2),
        prop::collection::vec(arb_type_arity(), 0..2),
        prop::collection::vec(arb_record_decl(), 0..3),
    )
        .prop_map(|(path, types, opaques, records)| HrlFile {
            path,
            types,
            opaques,
            records,
        })
}

fn arb_header() -> impl Strategy<Value = SnapshotHeader> {
    (
        "[a-z][a-z0-9_]{0,5}",
        "v[0-9]\\.[0-9]\\.[0-9]",
        prop::option::of("[a-z][a-z0-9._-]{0,8}"),
        "[0-9a-f]{40}",
    )
        .prop_map(|(project, tag, branch, commit)| SnapshotHeader {
            project: ProjectName::new(project).unwrap(),
            tag: TagName::new(tag).unwrap(),
            branch,
            commit: CommitSha::new(commit).unwrap(),
            scanned_paths: vec!["src".into()],
            apps_scanned: Vec::new(),
            generated_by: "backhopper 0.4.0".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        })
}

proptest! {
    #[test]
    fn arbitrary_snapshot_round_trips_through_writer_and_parser(
        header in arb_header(),
        modules in prop::collection::vec(arb_module(), 0..3),
        headers in prop::collection::vec(arb_hrl(), 0..3),
    ) {
        let snap = Snapshot::from_extracted(header, modules, headers).into_canonical();
        let text = format::to_string(&snap).unwrap();
        let back = parser::parse(&text).unwrap();
        prop_assert_eq!(snap, back);
    }

    #[test]
    fn snapshot_text_is_byte_stable_after_one_round_trip(
        header in arb_header(),
        modules in prop::collection::vec(arb_module(), 0..3),
        headers in prop::collection::vec(arb_hrl(), 0..3),
    ) {
        let snap = Snapshot::from_extracted(header, modules, headers).into_canonical();
        let text_a = format::to_string(&snap).unwrap();
        let back = parser::parse(&text_a).unwrap();
        let text_b = format::to_string(&back).unwrap();
        prop_assert_eq!(text_a, text_b);
    }

    #[test]
    fn multi_line_record_field_type_round_trips(
        alternatives in prop::collection::vec("[a-z][a-z0-9_]*\\(\\)", 2..5),
    ) {
        let multi = alternatives.join(" |\n      ");
        let mut hrl = HrlFile::new("include/x.hrl");
        hrl.records.push(RecordDecl {
            name: RecordName::new("r").unwrap(),
            fields: vec![RecordField {
                name: FieldName::new("a").unwrap(),
                type_repr: Some(multi),
            }],
        });
        let header = SnapshotHeader {
            project: ProjectName::new("p").unwrap(),
            tag: TagName::new("v0.1.0").unwrap(),
            branch: None,
            commit: CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: vec!["include".into()],
            apps_scanned: Vec::new(),
            generated_by: "backhopper".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        };
        let snap = Snapshot::from_extracted(header, vec![], vec![hrl]).into_canonical();
        let text = format::to_string(&snap).unwrap();
        let back = parser::parse(&text).unwrap();
        prop_assert_eq!(snap, back);
    }
}
