// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::str::FromStr;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::{FunArity, Module};

fn fa(name: &str, arity: u8) -> FunArity {
    FunArity {
        name: FunctionName::from_str(name).unwrap(),
        arity: Arity::new(arity),
    }
}

#[test]
fn empty_clause_heads_yields_no_findings() {
    let mut m = Module::new(ModuleName::new("x").unwrap());
    m.exports.push(fa("queue", 3));
    assert!(m.missing_clause_bodies().is_empty());
}

#[test]
fn matching_clause_head_yields_no_findings() {
    let mut m = Module::new(ModuleName::new("x").unwrap());
    let queue = fa("queue", 3);
    m.exports.push(queue.clone());
    m.clause_heads
        .insert(queue, vec![vec![ArgShape::Variable; 3]]);
    assert!(m.missing_clause_bodies().is_empty());
}

#[test]
fn export_without_body_is_reported() {
    let mut m = Module::new(ModuleName::new("rabbit_mgmt_wm_queue").unwrap());
    let missing = fa("queue", 3);
    m.exports.push(missing.clone());
    let other = fa("other", 1);
    m.exports.push(other.clone());
    let mut clause_heads: BTreeMap<FunArity, Vec<Vec<ArgShape>>> = BTreeMap::new();
    clause_heads.insert(other, vec![vec![ArgShape::Variable]]);
    m.clause_heads = clause_heads;
    let result = m.missing_clause_bodies();
    assert_eq!(result, vec![missing]);
}

#[test]
fn empty_exports_with_clause_heads_is_silent() {
    let mut m = Module::new(ModuleName::new("x").unwrap());
    m.clause_heads.insert(fa("internal", 0), vec![vec![]]);
    assert!(m.missing_clause_bodies().is_empty());
}
