// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Wire payloads for the `cache` verb group.
//!
//! The data behind these types is produced by `backhopper-cache`
//! (which depends on this crate); the shapes live here so the schema
//! generator and the driver see them without a dependency cycle.

use serde::{Deserialize, Serialize};

/// Which workspace cache an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CacheLevel {
    /// `.verdict_cache/by-input/`: SHA-keyed verdict entries.
    ByInput,
    /// `.verdict_cache/by-content/`: content-keyed verdict entries.
    ByContent,
    /// `.siblings_doctor_cache/`: `siblings doctor` run entries.
    Siblings,
}

impl CacheLevel {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ByInput => "by-input",
            Self::ByContent => "by-content",
            Self::Siblings => "siblings",
        }
    }
}

/// Per-level counters inside `cache stats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheLevelStats {
    pub entries: u64,
    pub bytes: u64,
}

/// The `cache stats` payload, covering both workspace caches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheStatsPayload {
    pub by_input: CacheLevelStats,
    pub by_content: CacheLevelStats,
    pub siblings: CacheLevelStats,
    /// `by-input` entries minted as aliases of a content-level hit.
    pub aliases: u64,
    pub total_bytes: u64,
    /// RFC3339 write times of the oldest and newest entries across
    /// every level; absent when all caches are empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_entry_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_entry_at: Option<String>,
}

/// One row in `cache list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheListRow {
    /// The entry's key hash (the file-name stem after the format
    /// prefix); unique prefixes of it address entries in `show` and
    /// `evict`.
    pub key: String,
    pub level: CacheLevel,
    /// The evaluated commit, for verdict entries that carry one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The series or pin target, when the pre-image names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    pub age_days: u32,
    pub bytes: u64,
    /// True for `by-input` entries minted from a content-level hit.
    #[serde(default)]
    pub alias: bool,
}

/// The `cache list` payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheListPayload {
    pub rows: Vec<CacheListRow>,
}

/// The `cache show` payload: one entry, pre-image included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheShowPayload {
    pub key: String,
    pub level: CacheLevel,
    pub created_at: String,
    pub age_days: u32,
    pub bytes: u64,
    /// True for `by-input` entries minted as aliases of a
    /// content-level hit.
    #[serde(default)]
    pub alias: bool,
    /// The canonical-JSON key pre-image stored inside the entry.
    pub key_inputs: serde_json::Value,
    /// Verdict counts for verdict entries; absent for siblings
    /// entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict_summary: Option<serde_json::Value>,
    /// The full stored value, under `--full` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

/// Payload for `cache evict`, `cache prune`, and `cache clear`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheMutationPayload {
    pub removed_entries: u64,
    pub removed_bytes: u64,
}
