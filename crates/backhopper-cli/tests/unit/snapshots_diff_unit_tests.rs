// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
    TypeName,
};
use backhopper_core::model::snapshot::state::Canonical;
use backhopper_core::model::snapshot::{
    CallbackSig, FunArity, HrlFile, Module, RecordDecl, RecordField, Snapshot, SnapshotHeader,
    TypeArity, TypeDecl,
};

use backhopper_cli::commands::snapshots::{DiffPayload, compute_diff, render_diff_text};

fn header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        generated_by: "backhopper test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
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
