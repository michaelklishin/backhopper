// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::ModuleName;
use backhopper_core::snapshot::format;
use backhopper_test_support::{canonical_snapshot, module_with, snapshot_header};

#[test]
fn write_module_filtered_returns_true_when_module_present() {
    let snap = canonical_snapshot(
        snapshot_header("p", "v1.0.0"),
        vec![
            module_with("alpha", &[("f", 1)]),
            module_with("beta", &[("g", 2)]),
        ],
    );
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
    let snap = canonical_snapshot(
        snapshot_header("p", "v1.0.0"),
        vec![module_with("alpha", &[("f", 1)])],
    );
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
    let snap = canonical_snapshot(
        snapshot_header("p", "v1.0.0"),
        vec![
            module_with("alpha", &[("f", 1)]),
            module_with("beta", &[("g", 2)]),
        ],
    );
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
    let snap = canonical_snapshot(
        snapshot_header("p", "v1.0.0"),
        vec![module_with("alpha", &[]), module_with("beta", &[])],
    );
    let mut buf = Vec::new();
    format::write_module_filtered(&snap, &ModuleName::new("beta").unwrap(), &mut buf)
        .expect("write");
    let text = String::from_utf8(buf).unwrap();
    assert_eq!(text.matches("\nmodule ").count(), 1);
}

#[test]
fn write_module_filtered_preserves_header() {
    let snap = canonical_snapshot(
        snapshot_header("p", "v1.0.0"),
        vec![module_with("alpha", &[])],
    );
    let mut buf = Vec::new();
    format::write_module_filtered(&snap, &ModuleName::new("alpha").unwrap(), &mut buf)
        .expect("write");
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("# project: p"));
    assert!(text.contains("# tag: v1.0.0"));
    assert!(text.contains(&format!(
        "# format-version: {}",
        backhopper_core::model::snapshot::FORMAT_VERSION
    )));
}
