// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::ApplicationName;
use backhopper_xref::XrefBuilder;

#[test]
fn empty_builder_produces_empty_xref() {
    let x = XrefBuilder::new().build().unwrap();
    assert_eq!(x.graph().module_count(), 0);
    assert!(x.warnings().is_empty());
}

#[test]
fn xref_clone_shares_underlying_graph() {
    let mut b = XrefBuilder::new();
    let app = ApplicationName::new("a".to_owned()).unwrap();
    let src = b"-module(a).\n-export([go/0]).\ngo() -> ok.\n".to_vec();
    b.add_application(app, vec![(PathBuf::from("a.erl"), src)])
        .unwrap();
    let x = b.build().unwrap();
    let y = x.clone();
    assert_eq!(x.graph().module_count(), y.graph().module_count());
    // Arc-backed: graph and warnings reference-equal.
    assert!(std::ptr::eq(x.graph(), y.graph()));
}

#[test]
fn duplicate_module_across_applications_emits_warning() {
    use backhopper_xref::ReadWarning;
    let mut b = XrefBuilder::new();
    let app_a = ApplicationName::new("a".to_owned()).unwrap();
    let app_b = ApplicationName::new("b".to_owned()).unwrap();
    let src = b"-module(shared).\n-export([f/0]).\nf() -> ok.\n".to_vec();
    b.add_application(app_a, vec![(PathBuf::from("a.erl"), src.clone())])
        .unwrap();
    b.add_application(app_b, vec![(PathBuf::from("a.erl"), src)])
        .unwrap();
    assert!(b.warnings().iter().any(|w| matches!(
        w,
        ReadWarning::DuplicateModuleAcrossApplications { name, .. }
            if name.as_str() == "shared"
    )));
}

#[test]
fn warnings_flow_through_to_xref() {
    let mut b = XrefBuilder::new();
    let app = ApplicationName::new("a".to_owned()).unwrap();
    b.add_application(app, vec![(PathBuf::from("empty.erl"), Vec::new())])
        .unwrap();
    let x = b.build().unwrap();
    assert!(!x.warnings().is_empty());
}

#[test]
fn add_application_collects_module_data() {
    let mut b = XrefBuilder::new();
    let app = ApplicationName::new("alpha".to_owned()).unwrap();
    let src = b"-module(a).\n-export([go/0]).\ngo() -> ok.\n".to_vec();
    b.add_application(app, vec![(PathBuf::from("a.erl"), src)])
        .unwrap();
    let x = b.build().unwrap();
    assert_eq!(x.graph().module_count(), 1);
}

#[test]
fn duplicate_application_returns_error() {
    let mut b = XrefBuilder::new();
    let app = ApplicationName::new("alpha".to_owned()).unwrap();
    b.add_application(app.clone(), Vec::new()).unwrap();
    let result = b.add_application(app, Vec::new());
    assert!(result.is_err());
}

#[test]
fn add_modules_inserts_from_iterator() {
    use backhopper_core::ModuleName;
    use backhopper_xref::ApplicationAssignment;
    use backhopper_xref::ModuleData;

    let m = ModuleData {
        module: ModuleName::new("synthesised".to_owned()).unwrap(),
        application: ApplicationAssignment::NoLayoutMatch {
            path: PathBuf::new(),
        },
        exports: Default::default(),
        locals: Default::default(),
        external_calls: Default::default(),
        local_calls: Default::default(),
        unresolved: Default::default(),
        on_load: None,
        deprecated: Default::default(),
        behaviours: Default::default(),
        callbacks: Default::default(),
        optional_callbacks: Default::default(),
        imports: Default::default(),
        definitions: Default::default(),
    };
    let mut b = XrefBuilder::new();
    b.add_modules(std::iter::once(m));
    let x = b.build().unwrap();
    assert_eq!(x.graph().module_count(), 1);
}
