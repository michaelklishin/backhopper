// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use backhopper_core::{ApplicationName, BehaviourName, ModuleName};
use backhopper_xref::{
    AppDependencies, ModuleCallers, ModuleDependencies, NonConformance, UnresolvedCalls,
};

fn mname(s: &str) -> ModuleName {
    ModuleName::new(s.to_owned()).unwrap()
}

#[test]
fn module_callers_display_lists_each_caller() {
    let cs: BTreeSet<ModuleName> = [mname("a"), mname("b")].into_iter().collect();
    let r = ModuleCallers {
        target: mname("lib"),
        entries: cs,
    };
    let out = format!("{r}");
    assert!(out.contains("callers of lib"));
    assert!(out.contains("  a"));
    assert!(out.contains("  b"));
}

#[test]
fn module_dependencies_display_uses_arrow_phrasing() {
    let cs: BTreeSet<ModuleName> = std::iter::once(mname("dep")).collect();
    let r = ModuleDependencies {
        source: mname("user"),
        entries: cs,
    };
    let out = format!("{r}");
    assert!(out.contains("user uses"));
    assert!(out.contains("  dep"));
}

#[test]
fn app_dependencies_display_uses_reaches_phrasing() {
    let cs: BTreeSet<ApplicationName> =
        std::iter::once(ApplicationName::new("dep".to_owned()).unwrap()).collect();
    let r = AppDependencies {
        source: ApplicationName::new("user".to_owned()).unwrap(),
        entries: cs,
    };
    let out = format!("{r}");
    assert!(out.contains("user reaches"));
    assert!(out.contains("  dep"));
}

#[test]
fn non_conformance_display_names_implementer_and_missing() {
    use backhopper_core::{Arity, FunctionName};
    use backhopper_xref::FunctionSig;
    let missing: BTreeSet<FunctionSig> = std::iter::once(FunctionSig::new(
        FunctionName::new("handle_call".to_owned()).unwrap(),
        Arity::new(3),
    ))
    .collect();
    let r = NonConformance {
        implementer: mname("my_server"),
        behaviour: BehaviourName::new("gen_server".to_owned()).unwrap(),
        missing,
    };
    let out = format!("{r}");
    assert!(out.contains("my_server"));
    assert!(out.contains("gen_server"));
    assert!(out.contains("handle_call/3"));
}

#[test]
fn unresolved_calls_display_empty_is_empty_string() {
    let r = UnresolvedCalls {
        entries: Vec::new(),
    };
    assert!(format!("{r}").is_empty());
}
