// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Generation of versioned envelope JSON-schemas.
//!
//! Behind the `schemars` feature so downstream consumers that don't
//! need schema metadata don't pay the build cost.

use schemars::{JsonSchema, schema_for};
use serde_json::{Value, json};
use thiserror::Error;

use crate::model::summary::SummaryRow;
use crate::model::verdict::SeriesEvaluation;

pub use crate::envelope_version::{
    CURRENT_SCHEMA_VERSION, MIN_EMBEDDED_VERSION, embedded_versions,
};

/// Frozen v1 schema bytes. Generated before `touched_paths` and
/// `pr_commits` were added to `SeriesEvaluation`; preserved so
/// `schema show 1` stays a stable archaeological record across binary
/// versions.
const SCHEMA_V1_FROZEN: &str = include_str!("schema_v1_snapshot.json");

/// Errors that can come out of schema generation.
#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("no schema embedded for version {requested}; known versions: {known:?}")]
    UnknownVersion { requested: u32, known: Vec<u32> },
}

/// Canonical JSON for one schema version: sorted keys, trailing
/// newline. Stable across regenerations so diffs stay informative.
pub fn rendered_schema(version: u32) -> Result<String, SchemaError> {
    let value = schema_value_for(version)?;
    let mut bytes = serde_json::to_vec_pretty(&value).expect("schemars output serialises");
    bytes.push(b'\n');
    Ok(String::from_utf8(bytes).expect("pretty json is utf-8"))
}

/// Structured `serde_json::Value` for one schema version. Used by
/// `schema diff` to compare two versions field-by-field.
pub fn schema_value_for(version: u32) -> Result<Value, SchemaError> {
    match version {
        1 => Ok(serde_json::from_str(SCHEMA_V1_FROZEN).expect("frozen v1 snapshot is valid JSON")),
        2 => Ok(envelope_with_payload::<SeriesEvaluation>(
            2,
            "check",
            "v2 adds `touched_paths: Vec<RelativePath>` and `pr_commits: \
             Option<Vec<PrCommit>>` to SeriesEvaluation. The check batch payload \
             (BatchPayload) was also promoted to a public type with the same fields.",
        )),
        3 => Ok(combined_v3()),
        4 => Ok(combined_v4()),
        5 => Ok(combined_v5()),
        other => Err(SchemaError::UnknownVersion {
            requested: other,
            known: embedded_versions(),
        }),
    }
}

fn combined_v4() -> Value {
    let mut v3 = combined_v3();
    if let Some(obj) = v3.as_object_mut() {
        obj.insert("schema_version".into(), json!(4));
        obj.insert("title".into(), json!("backhopper envelope v4"));
        obj.insert(
            "description".into(),
            json!(
                "v4 wires the path-translation pipeline: `Reason::PathRename` is emitted for \
                 translated paths and `InapplicableReason::PathsMissingOnTarget` now carries \
                 `Vec<RelativePath>` instead of `Vec<PathBuf>`. Reflected in the \
                 SeriesEvaluation payload."
            ),
        );
    }
    v3
}

fn combined_v5() -> Value {
    let series = envelope_with_payload::<SeriesEvaluation>(
        5,
        "check",
        "v5 adds the candidate-1, candidate-5, and candidate-6 surface from \
         backhopper/018: `Diagnostics.missing_test_modules` (always-on \
         counter-style diagnostic) and three new `Reason` variants \
         (`TestModuleSymbolMissing`, `BehaviourModuleMissing`, \
         `HeaderFileMissing`). Additive on shapes already declared \
         `#[non_exhaustive]`; older readers tolerate.",
    );
    let summary_row =
        serde_json::to_value(schema_for!(SummaryRow)).expect("SummaryRow schema serialises");
    let Value::Object(mut obj) = series else {
        unreachable!("envelope_with_payload always returns a Value::Object")
    };
    obj.insert("summary_row".into(), summary_row);
    obj.insert("schema_version".into(), json!(5));
    obj.insert("title".into(), json!("backhopper envelope v5"));
    Value::Object(obj)
}

fn combined_v3() -> Value {
    let series = envelope_with_payload::<SeriesEvaluation>(
        3,
        "check",
        "v3 captures the SummaryRow shape consumed by --formatter summary and \
         --formatter text-summary, alongside the v2 SeriesEvaluation surface.",
    );
    let summary_row =
        serde_json::to_value(schema_for!(SummaryRow)).expect("SummaryRow schema serialises");
    let Value::Object(mut obj) = series else {
        unreachable!("envelope_with_payload always returns a Value::Object")
    };
    obj.insert("summary_row".into(), summary_row);
    obj.insert("schema_version".into(), json!(3));
    obj.insert("title".into(), json!("backhopper envelope v3"));
    Value::Object(obj)
}

fn envelope_with_payload<T: JsonSchema>(
    version: u32,
    command_family: &str,
    description: &str,
) -> Value {
    let payload = serde_json::to_value(schema_for!(T)).expect("payload schema serialises");
    json!({
        "$schema": "https://json-schema.org/draft-07/schema",
        "title": format!("backhopper envelope v{version}"),
        "schema_version": version,
        "command_family": command_family,
        "description": description,
        "envelope": envelope_wrapper(payload),
    })
}

fn envelope_wrapper(payload_schema: Value) -> Value {
    json!({
        "type": "object",
        "required": ["schema_version", "command", "data", "exit_code"],
        "properties": {
            "schema_version": {
                "type": "integer",
                "minimum": 1,
                "description": "Wire-format schema version. Bumps when envelope shape changes."
            },
            "command": {
                "type": "string",
                "description": "Verb identifier in kebab-case, e.g. \"check merge\"."
            },
            "data": payload_schema,
            "exit_code": {
                "type": "integer",
                "description": "Process exit code carried inside the envelope; matches the process status."
            },
            "warnings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["code", "message"],
                    "properties": {
                        "code": { "type": "string" },
                        "message": { "type": "string" }
                    }
                },
                "description": "Free-form CLI warnings attached to the envelope."
            }
        }
    })
}
