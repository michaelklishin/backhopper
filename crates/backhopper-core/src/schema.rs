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
use crate::model::cache::{
    CacheListPayload, CacheMutationPayload, CacheShowPayload, CacheStatsPayload,
};
use crate::model::sibling_drift::SiblingDoctorReport;
use crate::model::summary::SummaryRow;
use crate::model::verdict::SeriesEvaluation;
use crate::suites::SuitePlan;

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
/// v2, v7, v8, and v9 froze when v10 changed the live types they
/// were generated from (`SeriesEvaluation` grew the already-present
/// and dep-pin diagnostics): historical versions must not drift.
const SCHEMA_V2_FROZEN: &str = include_str!("schema_v2_snapshot.json");
const SCHEMA_V7_FROZEN: &str = include_str!("schema_v7_snapshot.json");
const SCHEMA_V8_FROZEN: &str = include_str!("schema_v8_snapshot.json");
const SCHEMA_V9_FROZEN: &str = include_str!("schema_v9_snapshot.json");
const SCHEMA_V10_FROZEN: &str = include_str!("schema_v10_snapshot.json");

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
        2 => Ok(serde_json::from_str(SCHEMA_V2_FROZEN).expect("frozen v2 snapshot is valid JSON")),
        3 => Ok(serde_json::from_str(SCHEMA_V3_FROZEN).expect("frozen v3 snapshot is valid JSON")),
        4 => Ok(combined_v4()),
        5 => Ok(serde_json::from_str(SCHEMA_V5_FROZEN).expect("frozen v5 snapshot is valid JSON")),
        6 => Ok(combined_v6()),
        7 => Ok(serde_json::from_str(SCHEMA_V7_FROZEN).expect("frozen v7 snapshot is valid JSON")),
        8 => Ok(serde_json::from_str(SCHEMA_V8_FROZEN).expect("frozen v8 snapshot is valid JSON")),
        9 => Ok(serde_json::from_str(SCHEMA_V9_FROZEN).expect("frozen v9 snapshot is valid JSON")),
        10 => {
            Ok(serde_json::from_str(SCHEMA_V10_FROZEN).expect("frozen v10 snapshot is valid JSON"))
        }
        11 => Ok(combined_v11()),
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

fn combined_v11() -> Value {
    let series = envelope_with_payload::<SeriesEvaluation>(
        11,
        "check",
        "v11 connects tracked-dep snapshots to self-project call sites: path routing \
         becomes reason-granular (reference-level evidence survives for pins that own \
         no touched file), `Reason::MissingSymbol` gains `needs_pin_at_least` and a \
         populated `first_seen_at_tag`, `Reason::ArityChanged` gains \
         `expected_available_at` and `needs_pin_at_least` (both reclassify the reason \
         as adaptable: land the dep pin bump first), `SymbolRef` gains an `origin` \
         (`added` vs `context` hunk line) and the `function_any_arity` kind for calls \
         whose arity wraps to the next line, and `Diagnostics` gains \
         `context_refs_missing` plus `unattributed_paths`.",
    );
    let summary_row =
        serde_json::to_value(schema_for!(SummaryRow)).expect("SummaryRow schema serialises");
    let batch_payload =
        serde_json::to_value(schema_for!(BatchPayload)).expect("BatchPayload schema serialises");
    let siblings_doctor_payload = serde_json::to_value(schema_for!(SiblingDoctorReport))
        .expect("SiblingDoctorReport schema serialises");
    let cache_stats_payload = serde_json::to_value(schema_for!(CacheStatsPayload))
        .expect("CacheStatsPayload schema serialises");
    let cache_list_payload = serde_json::to_value(schema_for!(CacheListPayload))
        .expect("CacheListPayload schema serialises");
    let cache_show_payload = serde_json::to_value(schema_for!(CacheShowPayload))
        .expect("CacheShowPayload schema serialises");
    let cache_mutation_payload = serde_json::to_value(schema_for!(CacheMutationPayload))
        .expect("CacheMutationPayload schema serialises");
    let suite_plan_payload =
        serde_json::to_value(schema_for!(SuitePlan)).expect("SuitePlan schema serialises");
    let Value::Object(mut obj) = series else {
        unreachable!("envelope_with_payload always returns a Value::Object")
    };
    obj.insert("summary_row".into(), summary_row);
    obj.insert("batch_payload".into(), batch_payload);
    obj.insert("siblings_doctor_payload".into(), siblings_doctor_payload);
    obj.insert("cache_stats_payload".into(), cache_stats_payload);
    obj.insert("cache_list_payload".into(), cache_list_payload);
    obj.insert("cache_show_payload".into(), cache_show_payload);
    obj.insert("cache_mutation_payload".into(), cache_mutation_payload);
    obj.insert("suite_plan_payload".into(), suite_plan_payload);
    obj.insert("schema_version".into(), json!(11));
    obj.insert("title".into(), json!("backhopper envelope v11"));
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
