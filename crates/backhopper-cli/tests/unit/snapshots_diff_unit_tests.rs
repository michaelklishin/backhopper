// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FieldName, FunctionName, MacroName, ModuleName, ProjectName, RecordName,
    TagName, TypeName,
};
use backhopper_core::model::snapshot::state::Canonical;
use backhopper_core::model::snapshot::{
    CallbackSig, FunArity, HrlFile, Module, Provenance, RecordDecl, RecordField, Snapshot,
    SnapshotHeader, TypeArity, TypeDecl, VersionedMachineVersion, WireConstantBinding, WireValue,
};

use backhopper_cli::commands::snapshots::{compute_diff, render_diff_text};
use backhopper_core::model::snapshot_diff::{
    DiffPayload, VersionedMachineVersionChange, WireConstantChange,
};

fn header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

fn export(name: &str, arity: u8) -> FunArity {
    FunArity {
        name: FunctionName::new(name).unwrap(),
        arity: Arity::new(arity),
    }
}

fn module(name: &str, exports: Vec<FunArity>) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.exports = exports;
    m
}

fn module_with(name: &str, mut config: impl FnMut(&mut Module)) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    config(&mut m);
    m
}

fn snap(
    project: &str,
    tag: &str,
    modules: Vec<Module>,
    headers: Vec<HrlFile>,
) -> Snapshot<Canonical> {
    Snapshot::from_extracted(header(project, tag), modules, headers).into_canonical()
}

fn render(d: &DiffPayload) -> String {
    let mut buf = Vec::new();
    render_diff_text(&mut buf, d).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn seshat_overview_to_counters_diff_matches_documented_delta() {
    let a = snap(
        "seshat",
        "v0.6.1",
        vec![module(
            "seshat",
            vec![export("overview", 1), export("overview", 2)],
        )],
        vec![],
    );
    let b = snap(
        "seshat",
        "v1.0.1",
        vec![module(
            "seshat",
            vec![export("counters", 1), export("counters", 2)],
        )],
        vec![],
    );
    let d = compute_diff(&a, &b);
    let text = render(&d);
    assert!(text.contains("removed export seshat:overview/1"), "{text}");
    assert!(text.contains("removed export seshat:overview/2"), "{text}");
    assert!(text.contains("added export seshat:counters/1"), "{text}");
    assert!(text.contains("added export seshat:counters/2"), "{text}");
}

#[test]
fn modules_added_and_removed_appear_in_text() {
    let a = snap("p", "v0.1.0", vec![module("only_in_a", vec![])], vec![]);
    let b = snap("p", "v0.2.0", vec![module("only_in_b", vec![])], vec![]);
    let d = compute_diff(&a, &b);
    assert_eq!(d.modules_added, vec!["only_in_b"]);
    assert_eq!(d.modules_removed, vec!["only_in_a"]);
    let text = render(&d);
    assert!(text.contains("removed module only_in_a"), "{text}");
    assert!(text.contains("added module only_in_b"), "{text}");
}

#[test]
fn callbacks_diff_is_module_qualified() {
    let cb = |name: &str, arity: u8| CallbackSig {
        name: FunctionName::new(name).unwrap(),
        arity: Arity::new(arity),
        signature: "foo".into(),
    };
    let a = snap(
        "p",
        "v0.1.0",
        vec![module_with("m", |m| m.callbacks = vec![cb("init", 1)])],
        vec![],
    );
    let b = snap(
        "p",
        "v0.2.0",
        vec![module_with("m", |m| m.callbacks = vec![cb("init", 2)])],
        vec![],
    );
    let d = compute_diff(&a, &b);
    let text = render(&d);
    assert!(text.contains("removed callback m:init/1"), "{text}");
    assert!(text.contains("added callback m:init/2"), "{text}");
}

#[test]
fn type_decls_export_types_and_opaques_are_all_included_in_types_set() {
    let a = snap(
        "p",
        "v0.1.0",
        vec![module_with("m", |m| {
            m.types = vec![TypeDecl {
                name: TypeName::new("t1").unwrap(),
                arity: Arity::new(0),
                rhs: "any()".into(),
            }];
            m.export_types = vec![TypeArity {
                name: TypeName::new("et1").unwrap(),
                arity: Arity::new(0),
            }];
            m.opaques = vec![TypeArity {
                name: TypeName::new("op1").unwrap(),
                arity: Arity::new(0),
            }];
        })],
        vec![],
    );
    let b = snap("p", "v0.2.0", vec![module("m", vec![])], vec![]);
    let d = compute_diff(&a, &b);
    let text = render(&d);
    assert!(text.contains("removed type m:t1/0"), "{text}");
    assert!(text.contains("removed type m:et1/0"), "{text}");
    assert!(text.contains("removed type m:op1/0"), "{text}");
}

#[test]
fn headers_added_and_removed_appear_in_text() {
    let a = snap("p", "v0.1.0", vec![], vec![HrlFile::new("include/old.hrl")]);
    let b = snap("p", "v0.2.0", vec![], vec![HrlFile::new("include/new.hrl")]);
    let d = compute_diff(&a, &b);
    let text = render(&d);
    assert!(text.contains("removed header include/old.hrl"), "{text}");
    assert!(text.contains("added header include/new.hrl"), "{text}");
}

#[test]
fn records_added_and_removed_are_header_qualified() {
    let mut a_hrl = HrlFile::new("include/x.hrl");
    a_hrl.records.push(RecordDecl {
        name: RecordName::new("old_r").unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("f").unwrap(),
            type_repr: None,
        }],
    });
    let mut b_hrl = HrlFile::new("include/x.hrl");
    b_hrl.records.push(RecordDecl {
        name: RecordName::new("new_r").unwrap(),
        fields: vec![],
    });
    let a = snap("p", "v0.1.0", vec![], vec![a_hrl]);
    let b = snap("p", "v0.2.0", vec![], vec![b_hrl]);
    let d = compute_diff(&a, &b);
    let text = render(&d);
    assert!(
        text.contains("removed record include/x.hrl:old_r"),
        "{text}"
    );
    assert!(text.contains("added record include/x.hrl:new_r"), "{text}");
}

#[test]
fn equal_snapshots_produce_no_lines() {
    let m = module("m", vec![export("f", 1)]);
    let a = snap("p", "v0.1.0", vec![m.clone()], vec![]);
    let b = snap("p", "v0.2.0", vec![m], vec![]);
    let d = compute_diff(&a, &b);
    assert!(d.exports_added.is_empty());
    assert!(d.exports_removed.is_empty());
    assert!(d.modules_added.is_empty());
    assert!(d.modules_removed.is_empty());
    assert_eq!(render(&d), "");
}

#[test]
fn diff_json_serializes_with_all_buckets() {
    let a = snap(
        "p",
        "v0.1.0",
        vec![module("m", vec![export("f", 0)])],
        vec![],
    );
    let b = snap(
        "p",
        "v0.2.0",
        vec![module("m", vec![export("g", 0)])],
        vec![],
    );
    let d = compute_diff(&a, &b);
    let json = serde_json::to_string(&d).unwrap();
    for k in [
        "modules_added",
        "modules_removed",
        "exports_added",
        "exports_removed",
        "types_added",
        "types_removed",
        "callbacks_added",
        "callbacks_removed",
        "headers_added",
        "headers_removed",
        "records_added",
        "records_removed",
    ] {
        assert!(json.contains(k), "missing {k} in {json}");
    }
}

#[test]
fn rendering_orders_removed_before_added_inside_each_category() {
    let a = snap("p", "v1", vec![module("m", vec![export("a", 0)])], vec![]);
    let b = snap("p", "v2", vec![module("m", vec![export("z", 0)])], vec![]);
    let text = render(&compute_diff(&a, &b));
    let removed_idx = text.find("removed export").unwrap();
    let added_idx = text.find("added export").unwrap();
    assert!(removed_idx < added_idx, "{text}");
}

fn with_machine_version(name: &str, value: Option<u64>) -> Module {
    module_with(name, |m| {
        m.versioned_machine_version = Some(VersionedMachineVersion {
            function: FunctionName::new("version").unwrap(),
            arity: Arity::new(0),
            value,
            provenance: Provenance::Literal,
        });
    })
}

fn with_wire_constants(name: &str, macros: &[(&str, u64)]) -> Module {
    module_with(name, |m| {
        m.wire_constants = macros
            .iter()
            .map(|(macro_name, value)| WireConstantBinding {
                macro_name: MacroName::new(*macro_name).unwrap(),
                value: WireValue::U64(*value),
                defined_in: None,
            })
            .collect();
    })
}

#[test]
fn versioned_machine_version_missing_on_from_side() {
    let a = snap("p", "v0", vec![module("rabbit_fifo", vec![])], vec![]);
    let b = snap(
        "p",
        "v1",
        vec![with_machine_version("rabbit_fifo", Some(7))],
        vec![],
    );
    let d = compute_diff(&a, &b);
    assert_eq!(d.versioned_machine_version_changes.len(), 1);
    assert!(matches!(
        &d.versioned_machine_version_changes[0],
        VersionedMachineVersionChange::Missing { module, side }
            if module == "rabbit_fifo" && side == "from"
    ));
    let text = render(&d);
    assert!(
        text.contains("versioned_machine_version missing on from rabbit_fifo"),
        "{text}"
    );
}

#[test]
fn versioned_machine_version_drift_emits_kind_drift() {
    let a = snap(
        "p",
        "v0",
        vec![with_machine_version("rabbit_fifo", Some(7))],
        vec![],
    );
    let b = snap(
        "p",
        "v1",
        vec![with_machine_version("rabbit_fifo", Some(8))],
        vec![],
    );
    let d = compute_diff(&a, &b);
    assert_eq!(d.versioned_machine_version_changes.len(), 1);
    assert!(matches!(
        &d.versioned_machine_version_changes[0],
        VersionedMachineVersionChange::Drift {
            module, from, to,
        } if module == "rabbit_fifo" && *from == Some(7) && *to == Some(8)
    ));
}

#[test]
fn equal_versioned_machine_versions_produce_no_rows() {
    let a = snap(
        "p",
        "v0",
        vec![with_machine_version("rabbit_fifo", Some(7))],
        vec![],
    );
    let b = snap(
        "p",
        "v1",
        vec![with_machine_version("rabbit_fifo", Some(7))],
        vec![],
    );
    let d = compute_diff(&a, &b);
    assert!(d.versioned_machine_version_changes.is_empty());
}

#[test]
fn wire_constant_missing_groups_by_side_and_sorts_macros() {
    let a = snap(
        "p",
        "v0",
        vec![with_wire_constants(
            "ra_log_segment",
            &[("VERSION", 1), ("MAGIC", 2)],
        )],
        vec![],
    );
    let b = snap(
        "p",
        "v1",
        vec![with_wire_constants("ra_log_segment", &[("VERSION", 1)])],
        vec![],
    );
    let d = compute_diff(&a, &b);
    let missing: Vec<_> = d
        .wire_constant_changes
        .iter()
        .filter_map(|c| match c {
            WireConstantChange::Missing {
                module,
                side,
                macros,
            } => Some((module.clone(), side.clone(), macros.clone())),
            WireConstantChange::Drift { .. } => None,
        })
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].0, "ra_log_segment");
    assert_eq!(missing[0].1, "to");
    assert_eq!(missing[0].2, vec!["MAGIC".to_owned()]);
}

#[test]
fn wire_constant_value_drift_emits_one_row_per_macro() {
    let a = snap(
        "p",
        "v0",
        vec![with_wire_constants("ra_log_segment", &[("VERSION", 1)])],
        vec![],
    );
    let b = snap(
        "p",
        "v1",
        vec![with_wire_constants("ra_log_segment", &[("VERSION", 2)])],
        vec![],
    );
    let d = compute_diff(&a, &b);
    let drifts: Vec<_> = d
        .wire_constant_changes
        .iter()
        .filter_map(|c| match c {
            WireConstantChange::Drift {
                module,
                macro_name,
                from,
                to,
            } => Some((module.clone(), macro_name.clone(), from.clone(), to.clone())),
            WireConstantChange::Missing { .. } => None,
        })
        .collect();
    assert_eq!(drifts.len(), 1);
    assert_eq!(drifts[0].0, "ra_log_segment");
    assert_eq!(drifts[0].1, "VERSION");
    assert_eq!(drifts[0].2, "u64 1");
    assert_eq!(drifts[0].3, "u64 2");
}

#[test]
fn diff_payload_serializes_without_new_fields_when_empty() {
    let a = snap("p", "v0", vec![module("m", vec![])], vec![]);
    let b = snap("p", "v1", vec![module("m", vec![])], vec![]);
    let d = compute_diff(&a, &b);
    let json = serde_json::to_string(&d).unwrap();
    assert!(
        !json.contains("versioned_machine_version_changes"),
        "{json}"
    );
    assert!(!json.contains("wire_constant_changes"), "{json}");
}
