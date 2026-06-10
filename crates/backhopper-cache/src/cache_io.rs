// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Content-addressed cache entries.
//!
//! Every entry is one JSON file whose name is derived from the
//! canonical-JSON form of its key, so a lookup never scans the
//! directory. The file embeds a freshness document; a lookup whose
//! freshness no longer matches is a miss, and the next store
//! overwrites the entry in place. Writes go through a temp file plus
//! an atomic rename so a crashed process can never leave a torn entry.

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::errors::CacheError;

/// On-disk entry layout version. Carried both inside the entry and as
/// the file-name prefix (`v1-<hash>.json`) so a future layout change
/// can prune stale entries by prefix without parsing them.
pub const ENTRY_FORMAT_VERSION: u32 = 1;

/// Hex characters of the BLAKE3 key hash kept in the file name:
/// 128 bits, far beyond collision concerns for a per-workspace cache.
const KEY_HASH_LEN: usize = 32;

/// Canonical JSON bytes for `value`: object keys sorted, no
/// insignificant whitespace. Stable across runs, so hashes of equal
/// keys always collide and nothing else does.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CacheError> {
    // Round-tripping through Value sorts object keys: serde_json's
    // Value::Object is a BTreeMap.
    let normalised: Value = serde_json::to_value(value)?;
    Ok(serde_json::to_vec(&normalised)?)
}

/// BLAKE3 hex digest of the canonical-JSON form of `value`, truncated
/// to a length that still rules out collisions in practice.
pub fn content_hash<T: Serialize>(value: &T) -> Result<String, CacheError> {
    let bytes = canonical_json(value)?;
    let mut hex = blake3::hash(&bytes).to_hex().to_string();
    hex.truncate(KEY_HASH_LEN);
    Ok(hex)
}

/// One cache directory holding entries of a single logical kind
/// (e.g. `.siblings_doctor_cache/`).
#[derive(Debug, Clone)]
pub struct CacheDir {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct EntryEnvelope<V> {
    entry_format_version: u32,
    written_at: String,
    key: Value,
    freshness: Value,
    value: V,
}

impl CacheDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path the entry for `key` lives at (whether or not it exists).
    pub fn entry_path<K: Serialize>(&self, key: &K) -> Result<PathBuf, CacheError> {
        let hash = content_hash(key)?;
        Ok(self
            .root
            .join(format!("v{ENTRY_FORMAT_VERSION}-{hash}.json")))
    }

    /// Serve the stored value for `key` when the entry exists, parses,
    /// and its stored freshness document equals `freshness`. Every
    /// other outcome (absent, torn, foreign layout, stale) is a miss.
    pub fn lookup<K, F, V>(&self, key: &K, freshness: &F) -> Result<Option<V>, CacheError>
    where
        K: Serialize,
        F: Serialize,
        V: DeserializeOwned,
    {
        let path = self.entry_path(key)?;
        let Ok(bytes) = fs::read(&path) else {
            return Ok(None);
        };
        let Ok(entry) = serde_json::from_slice::<EntryEnvelope<V>>(&bytes) else {
            tracing::debug!(path = %path.display(), "cache entry unparsable; treating as a miss");
            return Ok(None);
        };
        if entry.entry_format_version != ENTRY_FORMAT_VERSION {
            return Ok(None);
        }
        let expected: Value = serde_json::to_value(freshness)?;
        if entry.freshness != expected {
            tracing::debug!(path = %path.display(), "cache entry stale; freshness mismatch");
            return Ok(None);
        }
        Ok(Some(entry.value))
    }

    /// Write (or overwrite) the entry for `key`. Returns the entry
    /// path. The write is atomic: temp file in the same directory,
    /// then rename.
    pub fn store<K, F, V>(&self, key: &K, freshness: &F, value: &V) -> Result<PathBuf, CacheError>
    where
        K: Serialize,
        F: Serialize,
        V: Serialize,
    {
        let path = self.entry_path(key)?;
        fs::create_dir_all(&self.root)?;
        let entry = EntryEnvelope {
            entry_format_version: ENTRY_FORMAT_VERSION,
            written_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default(),
            key: serde_json::to_value(key)?,
            freshness: serde_json::to_value(freshness)?,
            value,
        };
        let bytes = serde_json::to_vec_pretty(&entry)?;
        let tmp = path.with_extension(format!("json.{}.tmp", process::id()));
        fs::write(&tmp, &bytes)?;
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(CacheError::Io(e));
        }
        Ok(path)
    }
}
