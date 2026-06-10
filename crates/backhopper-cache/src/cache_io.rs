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
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime};

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
    Ok(truncated_hex(blake3::hash(&bytes)))
}

/// BLAKE3 hex digest of raw bytes, same truncation as `content_hash`.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    truncated_hex(blake3::hash(bytes))
}

/// BLAKE3 hex digest of a file's bytes, streamed without parsing.
pub fn hash_file(path: &Path) -> Result<String, CacheError> {
    let mut hasher = blake3::Hasher::new();
    let mut file = fs::File::open(path)?;
    io::copy(&mut file, &mut hasher)?;
    Ok(truncated_hex(hasher.finalize()))
}

fn truncated_hex(hash: blake3::Hash) -> String {
    let mut hex = hash.to_hex().to_string();
    hex.truncate(KEY_HASH_LEN);
    hex
}

/// File name for an entry addressed by `hash`, carrying the layout
/// version as a prune-safe prefix.
pub(crate) fn entry_file_name(hash: &str) -> String {
    format!("v{ENTRY_FORMAT_VERSION}-{hash}.json")
}

/// True for file names shaped like cache entries (`v<N>-<hex>.json`).
/// Sweeps and prunes only ever delete names this accepts, so a cache
/// directory misconfigured onto real data cannot lose it.
#[must_use]
pub fn is_entry_file_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('v') else {
        return false;
    };
    let Some((version, hash_json)) = rest.split_once('-') else {
        return false;
    };
    let Some(hash) = hash_json.strip_suffix(".json") else {
        return false;
    };
    !version.is_empty()
        && version.chars().all(|c| c.is_ascii_digit())
        && hash.len() >= 16
        && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// Write `bytes` to `path` via a same-directory temp file and an
/// atomic rename, so readers never observe a torn entry.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    let tmp = path.with_extension(format!("json.{}.tmp", process::id()));
    fs::write(&tmp, bytes)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(CacheError::Io(e));
    }
    Ok(())
}

/// True when the file at `path` is older than `max_age`. Unreadable
/// metadata reads as not expired (the parse path decides what to do
/// with the entry).
pub(crate) fn is_older_than(path: &Path, max_age: Duration) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age > max_age)
}

/// One cache directory holding entries of a single logical kind
/// (e.g. `.siblings_doctor_cache/`).
#[derive(Debug, Clone)]
pub struct CacheDir {
    root: PathBuf,
    max_age: Option<Duration>,
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
        Self {
            root: root.into(),
            max_age: None,
        }
    }

    /// Adopt a TTL: lookups treat entries older than `max_age` as
    /// misses and delete them. `None` disables expiry.
    pub fn with_max_age(mut self, max_age: Option<Duration>) -> Self {
        self.max_age = max_age;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path the entry for `key` lives at (whether or not it exists).
    pub fn entry_path<K: Serialize>(&self, key: &K) -> Result<PathBuf, CacheError> {
        let hash = content_hash(key)?;
        Ok(self.root.join(entry_file_name(&hash)))
    }

    /// Serve the stored value for `key` when the entry exists, parses,
    /// is younger than any configured TTL, and its stored freshness
    /// document equals `freshness`. Every other outcome (absent, torn,
    /// foreign layout, expired, stale) is a miss.
    pub fn lookup<K, F, V>(&self, key: &K, freshness: &F) -> Result<Option<V>, CacheError>
    where
        K: Serialize,
        F: Serialize,
        V: DeserializeOwned,
    {
        let path = self.entry_path(key)?;
        if let Some(max_age) = self.max_age
            && is_older_than(&path, max_age)
        {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
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
        write_atomic(&path, &bytes)?;
        Ok(path)
    }
}
