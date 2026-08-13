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
use std::path::Path;
use std::process;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

use crate::errors::CacheError;

/// On-disk entry layout version. Carried both inside the entry and as
/// the file-name prefix (`v1-<hash>.json`) so a future layout change
/// can prune stale entries by prefix without parsing them.
pub const ENTRY_FORMAT_VERSION: u32 = 1;

/// Hex characters of the BLAKE3 key hash kept in the file name:
/// 128 bits, far beyond collision concerns for a per-workspace cache.
const KEY_HASH_LEN: usize = 32;

/// Canonical JSON bytes for `value`: object keys sorted, no
/// insignificant whitespace. Stable across runs, so equal keys
/// always hash equal and unequal keys do not.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CacheError> {
    // round-tripping through Value sorts object keys: Value::Object is a BTreeMap
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
