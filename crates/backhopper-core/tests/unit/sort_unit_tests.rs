// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::{FunArity, HrlFile, Module};
use backhopper_core::snapshot::sort::canonicalize;

fn fa(name: &str, arity: u8) -> FunArity {
    FunArity {
        name: FunctionName::new(name).unwrap(),
        arity: Arity::new(arity),
    }
}

#[test]
fn canonicalize_orders_modules_alphabetically() {
    let mut modules = vec![
        Module::new(ModuleName::new("zoo").unwrap()),
        Module::new(ModuleName::new("aardvark").unwrap()),
        Module::new(ModuleName::new("middle").unwrap()),
    ];
    let mut headers: Vec<HrlFile> = Vec::new();
    canonicalize(&mut modules, &mut headers);
    assert_eq!(modules[0].name.as_str(), "aardvark");
    assert_eq!(modules[1].name.as_str(), "middle");
    assert_eq!(modules[2].name.as_str(), "zoo");
}

#[test]
fn canonicalize_orders_exports_by_name_then_arity() {
    let mut m = Module::new(ModuleName::new("ra").unwrap());
    m.exports.push(fa("process_command", 3));
    m.exports.push(fa("init", 1));
    m.exports.push(fa("process_command", 2));
    let mut modules = vec![m];
    let mut headers: Vec<HrlFile> = Vec::new();
    canonicalize(&mut modules, &mut headers);
    assert_eq!(
        modules[0]
            .exports
            .iter()
            .map(|fa| format!("{}/{}", fa.name, fa.arity))
            .collect::<Vec<_>>(),
        vec!["init/1", "process_command/2", "process_command/3"]
    );
}

#[test]
fn canonicalize_is_idempotent() {
    let mut m = Module::new(ModuleName::new("ra").unwrap());
    m.exports.push(fa("b", 1));
    m.exports.push(fa("a", 2));
    m.exports.push(fa("a", 1));
    let mut modules = vec![m];
    let mut headers: Vec<HrlFile> = Vec::new();
    canonicalize(&mut modules, &mut headers);
    let snapshot1 = modules.clone();
    canonicalize(&mut modules, &mut headers);
    assert_eq!(snapshot1, modules);
}

#[test]
fn canonicalize_dedupes_consecutive_duplicates_in_exports() {
    let mut m = Module::new(ModuleName::new("ra").unwrap());
    m.exports.push(fa("init", 1));
    m.exports.push(fa("init", 1));
    let mut modules = vec![m];
    let mut headers: Vec<HrlFile> = Vec::new();
    canonicalize(&mut modules, &mut headers);
    assert_eq!(modules[0].exports.len(), 1);
}

#[test]
fn canonicalize_dedupes_duplicate_modules() {
    let mut modules = vec![
        Module::new(ModuleName::new("ra").unwrap()),
        Module::new(ModuleName::new("ra").unwrap()),
    ];
    let mut headers: Vec<HrlFile> = Vec::new();
    canonicalize(&mut modules, &mut headers);
    assert_eq!(modules.len(), 1);
}

#[test]
fn canonicalize_dedupes_behaviours() {
    let mut m = Module::new(ModuleName::new("ra").unwrap());
    m.behaviours.push(ModuleName::new("gen_server").unwrap());
    m.behaviours.push(ModuleName::new("gen_server").unwrap());
    let mut modules = vec![m];
    let mut headers: Vec<HrlFile> = Vec::new();
    canonicalize(&mut modules, &mut headers);
    assert_eq!(modules[0].behaviours.len(), 1);
}
