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

use crate::model::batch::BatchPayload;
use crate::model::sibling_drift::SiblingDoctorReport;
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

/// Frozen v3 and v5 schema bytes. Generated before v7 added `series`
/// and `parent_count` to `SummaryRow`; frozen so older versions stay
/// stable archaeological records instead of silently absorbing later
/// type changes. v4 and v6 derive from these (they only retitle).
const SCHEMA_V3_FROZEN: &str = include_str!("schema_v3_snapshot.json");
const SCHEMA_V5_FROZEN: &str = include_str!("schema_v5_snapshot.json");

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
        3 => Ok(serde_json::from_str(SCHEMA_V3_FROZEN).expect("frozen v3 snapshot is valid JSON")),
        4 => Ok(combined_v4()),
        5 => Ok(serde_json::from_str(SCHEMA_V5_FROZEN).expect("frozen v5 snapshot is valid JSON")),
        6 => Ok(combined_v6()),
        7 => Ok(combined_v7()),
        8 => Ok(combined_v8()),
        other => Err(SchemaError::UnknownVersion {
            requested: other,
            known: embedded_versions(),
        }),
    }
}

fn combined_v4() -> Value {
    let mut v3: Value =
        serde_json::from_str(SCHEMA_V3_FROZEN).expect("frozen v3 snapshot is valid JSON");
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

fn combined_v6() -> Value {
    let mut v5: Value =
        serde_json::from_str(SCHEMA_V5_FROZEN).expect("frozen v5 snapshot is valid JSON");
    if let Some(obj) = v5.as_object_mut() {
        obj.insert("schema_version".into(), json!(6));
        obj.insert("title".into(), json!("backhopper envelope v6"));
        obj.insert(
            "description".into(),
            json!(
                "v6: operator-facing SHA inputs accept 7-to-40 hex prefixes (resolved through \
                 gix before any analyser sees them), new typed `GitError::AmbiguousSha` and \
                 `GitError::NotACommit` variants surface through the error path, the \
                 `rev resolve` companion verb expands a prefix to a full SHA, and \
                 `BisectPayload.commit` plus `MultiPayload.commit` carry typed `CommitSha` \
                 rather than raw strings."
            ),
        );
    }
    v5
}

fn combined_v7() -> Value {
    let series = envelope_with_payload::<SeriesEvaluation>(
        7,
        "check",
        "v7 makes `check batch` and `check multi` merge-aware: 2-parent merge SHAs \
         evaluate as the first-parent diff with `pr_commits` populated, octopus merges \
         evaluate first-parent with `pr_commits: null`. `BatchResult` gains \
         `parent_count` (1 plain, 2 PR merge, 3+ octopus; null only from pre-v7 \
         binaries). `SummaryRow` gains `series` and `parent_count`, and the summary \
         formatters now emit one row per (commit, series) pair on batch and multi.",
    );
    let summary_row =
        serde_json::to_value(schema_for!(SummaryRow)).expect("SummaryRow schema serialises");
    let batch_payload =
        serde_json::to_value(schema_for!(BatchPayload)).expect("BatchPayload schema serialises");
    let Value::Object(mut obj) = series else {
        unreachable!("envelope_with_payload always returns a Value::Object")
    };
    obj.insert("summary_row".into(), summary_row);
    obj.insert("batch_payload".into(), batch_payload);
    obj.insert("schema_version".into(), json!(7));
    obj.insert("title".into(), json!("backhopper envelope v7"));
    Value::Object(obj)
}

fn combined_v8() -> Value {
    let series = envelope_with_payload::<SeriesEvaluation>(
        8,
        "check",
        "v8 adds the `siblings doctor` verb: a ranked report of sibling-branch commits \
         that look like they should have cascaded to a series but never did \
         (`siblings_doctor_payload`), with `-x` trailer plus patch-id suppression of \
         already-cascaded fixes and a typed `since` derivation. The `version` payload \
         gains a `verbs` capability list so drivers probe verb presence by name instead \
         of inferring it from the schema number.",
    );
    let summary_row =
        serde_json::to_value(schema_for!(SummaryRow)).expect("SummaryRow schema serialises");
    let batch_payload =
        serde_json::to_value(schema_for!(BatchPayload)).expect("BatchPayload schema serialises");
    let siblings_doctor_payload = serde_json::to_value(schema_for!(SiblingDoctorReport))
        .expect("SiblingDoctorReport schema serialises");
    let Value::Object(mut obj) = series else {
        unreachable!("envelope_with_payload always returns a Value::Object")
    };
    obj.insert("summary_row".into(), summary_row);
    obj.insert("batch_payload".into(), batch_payload);
    obj.insert("siblings_doctor_payload".into(), siblings_doctor_payload);
    obj.insert("schema_version".into(), json!(8));
    obj.insert("title".into(), json!("backhopper envelope v8"));
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
