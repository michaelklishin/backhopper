// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The canonical JSON envelope frame every verb wraps its payload in.
//!
//! One definition the CLI serializes, the driver parses, and `schema
//! show` documents, so the frame cannot drift across the three. The
//! payload type `T` is the per-verb body.

use serde::{Deserialize, Serialize};

/// A non-fatal warning carried alongside a payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct EnvelopeWarning {
    /// Stable warning code, kebab-case (e.g. `stale-extractor`).
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// The `{schema_version, command, data, exit_code[, warnings]}` frame.
///
/// `command` and `warnings` are lenient on the read side (an older
/// producer may omit either); `warnings` is dropped from the wire when
/// empty, matching the CLI, which does not emit it today.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WireEnvelope<T> {
    pub schema_version: u32,
    #[serde(default)]
    pub command: Option<String>,
    pub data: T,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<EnvelopeWarning>,
}
