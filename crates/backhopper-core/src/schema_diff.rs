// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Structured diff between two schema versions.
//!
//! The diff walks two `serde_json::Value` trees in parallel, emitting
//! JSON-Pointer-keyed differences. Suitable for agents migrating
//! between binary releases.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SchemaDiff {
    pub from: u32,
    pub to: u32,
    pub added_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub changed_types: Vec<TypeChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TypeChange {
    pub path: String,
    pub old_type: String,
    pub new_type: String,
}

/// Walk `from` and `to` schema values and emit a structured diff.
///
/// `added_paths` are JSON Pointer paths present in `to` but not in
/// `from`. `removed_paths` are the inverse. `changed_types` flags any
/// terminal-value type changes (e.g., `"integer"` to `"string"` at
/// the same path).
#[must_use]
pub fn diff(from_version: u32, to_version: u32, from: &Value, to: &Value) -> SchemaDiff {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    walk(from, to, "", &mut added, &mut removed, &mut changed);
    added.sort();
    removed.sort();
    changed.sort_by(|a, b| a.path.cmp(&b.path));
    SchemaDiff {
        from: from_version,
        to: to_version,
        added_paths: added,
        removed_paths: removed,
        changed_types: changed,
    }
}

fn walk(
    from: &Value,
    to: &Value,
    path: &str,
    added: &mut Vec<String>,
    removed: &mut Vec<String>,
    changed: &mut Vec<TypeChange>,
) {
    match (from, to) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let child_path = format!("{path}/{}", escape_pointer_segment(key));
                match (a.get(key), b.get(key)) {
                    (None, Some(_)) => added.push(child_path),
                    (Some(_), None) => removed.push(child_path),
                    (Some(av), Some(bv)) => walk(av, bv, &child_path, added, removed, changed),
                    (None, None) => unreachable!("key from union"),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            let max = a.len().max(b.len());
            for idx in 0..max {
                let child_path = format!("{path}/{idx}");
                match (a.get(idx), b.get(idx)) {
                    (None, Some(_)) => added.push(child_path),
                    (Some(_), None) => removed.push(child_path),
                    (Some(av), Some(bv)) => walk(av, bv, &child_path, added, removed, changed),
                    (None, None) => unreachable!("index in range"),
                }
            }
        }
        (a, b) if json_type_name(a) != json_type_name(b) => {
            changed.push(TypeChange {
                path: path.to_owned(),
                old_type: json_type_name(a).to_owned(),
                new_type: json_type_name(b).to_owned(),
            });
        }
        _ => {}
    }
}

fn escape_pointer_segment(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
