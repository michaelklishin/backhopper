// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The inspection surface behind the `cache` verb group: scanning,
//! stats, key-prefix resolution, eviction, pruning, and clearing,
//! across both workspace caches (the verdict cache and the siblings
//! doctor cache).
//!
//! The caches hold a few hundred entries, so scan-and-filter is the
//! index: every operation parses the self-describing entries instead
//! of maintaining a side database.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use backhopper_core::model::cache::{CacheLevel, CacheStatsPayload};

use crate::cache_io::is_entry_file_name;
use crate::sweep::sweep_dir;
use crate::verdict::{SIBLINGS_CACHE_DIR_NAME, VERDICT_CACHE_DIR_NAME};

/// The cache directories of one snapshot workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceCaches {
    pub verdict_by_input: PathBuf,
    pub verdict_by_content: PathBuf,
    pub siblings: PathBuf,
}

impl WorkspaceCaches {
    #[must_use]
    pub fn at(snapshot_dir: &Path) -> Self {
        let verdict = snapshot_dir.join(VERDICT_CACHE_DIR_NAME);
        Self {
            verdict_by_input: verdict.join("by-input"),
            verdict_by_content: verdict.join("by-content"),
            siblings: snapshot_dir.join(SIBLINGS_CACHE_DIR_NAME),
        }
    }

    fn levels(&self) -> [(&Path, CacheLevel); 3] {
        [
            (self.verdict_by_input.as_path(), CacheLevel::ByInput),
            (self.verdict_by_content.as_path(), CacheLevel::ByContent),
            (self.siblings.as_path(), CacheLevel::Siblings),
        ]
    }
}

/// One parsed cache entry, normalized across the two entry layouts
/// (verdict entries and the generic `cache_io` envelope).
#[derive(Debug, Clone)]
pub struct ScannedEntry {
    /// The hex key: the file-name stem between the format prefix and
    /// `.json`.
    pub key: String,
    pub level: CacheLevel,
    pub path: PathBuf,
    pub bytes: u64,
    pub age: Duration,
    pub created_at: Option<String>,
    pub key_inputs: Value,
    pub alias: bool,
    /// The stored value: the evaluation for verdict entries, the run
    /// payload for siblings entries.
    pub value: Value,
}

impl ScannedEntry {
    /// The evaluated commit, for entries whose pre-image names one.
    #[must_use]
    pub fn commit(&self) -> Option<&str> {
        self.key_inputs.get("commit").and_then(Value::as_str)
    }

    /// The series, for entries whose pre-image names one.
    #[must_use]
    pub fn series(&self) -> Option<&str> {
        self.key_inputs.get("series").and_then(Value::as_str)
    }
}

/// Parse every entry across the workspace caches, in (level, key)
/// order. Unparseable files are skipped, never deleted: inspection
/// is read-only.
#[must_use]
pub fn scan(caches: &WorkspaceCaches) -> Vec<ScannedEntry> {
    let mut out: Vec<ScannedEntry> = Vec::new();
    for (dir, level) in caches.levels() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(scanned) = scan_one(&path, level) {
                out.push(scanned);
            }
        }
    }
    out.sort_by(|a, b| (level_rank(a.level), &a.key).cmp(&(level_rank(b.level), &b.key)));
    out
}

// presentation order: the fast path first, then content, then siblings
fn level_rank(level: CacheLevel) -> u8 {
    match level {
        CacheLevel::ByInput => 0,
        CacheLevel::ByContent => 1,
        CacheLevel::Siblings => 2,
    }
}

fn scan_one(path: &Path, level: CacheLevel) -> Option<ScannedEntry> {
    let name = path.file_name()?.to_str()?;
    if !is_entry_file_name(name) {
        return None;
    }
    let key = name.split_once('-')?.1.strip_suffix(".json")?.to_owned();
    let meta = fs::metadata(path).ok()?;
    let age = meta
        .modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .unwrap_or_default();
    let doc: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    // verdict entries carry key_inputs/created_at/evaluation; the
    // generic envelope carries key/written_at/value
    let key_inputs = doc
        .get("key_inputs")
        .or_else(|| doc.get("key"))
        .cloned()
        .unwrap_or(Value::Null);
    let created_at = doc
        .get("created_at")
        .or_else(|| doc.get("written_at"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let alias = doc
        .get("alias_of_content_key")
        .is_some_and(|v| !v.is_null());
    let value = doc
        .get("evaluation")
        .or_else(|| doc.get("value"))
        .cloned()
        .unwrap_or(Value::Null);
    Some(ScannedEntry {
        key,
        level,
        path: path.to_path_buf(),
        bytes: meta.len(),
        age,
        created_at,
        key_inputs,
        alias,
        value,
    })
}

/// Entry counts, sizes, alias count, and the write-time range across
/// every level.
#[must_use]
pub fn stats(caches: &WorkspaceCaches) -> CacheStatsPayload {
    let entries = scan(caches);
    let mut payload = CacheStatsPayload::default();
    let mut oldest: Option<SystemTime> = None;
    let mut newest: Option<SystemTime> = None;
    let now = SystemTime::now();
    for entry in &entries {
        let level = match entry.level {
            CacheLevel::ByInput => &mut payload.by_input,
            CacheLevel::ByContent => &mut payload.by_content,
            CacheLevel::Siblings => &mut payload.siblings,
        };
        level.entries += 1;
        level.bytes += entry.bytes;
        payload.total_bytes += entry.bytes;
        if entry.alias {
            payload.aliases += 1;
        }
        let written = now.checked_sub(entry.age).unwrap_or(now);
        if oldest.is_none_or(|o| written < o) {
            oldest = Some(written);
        }
        if newest.is_none_or(|n| written > n) {
            newest = Some(written);
        }
    }
    payload.oldest_entry_at = oldest.map(format_system_time);
    payload.newest_entry_at = newest.map(format_system_time);
    payload
}

fn format_system_time(t: SystemTime) -> String {
    OffsetDateTime::from(t).format(&Rfc3339).unwrap_or_default()
}

/// How a key prefix failed to resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyLookupError {
    NotFound,
    /// Sorted keys of every entry the prefix matched.
    Ambiguous(Vec<String>),
}

/// Resolve a unique key prefix against scanned entries.
pub fn find_by_key_prefix<'a>(
    entries: &'a [ScannedEntry],
    prefix: &str,
) -> Result<&'a ScannedEntry, KeyLookupError> {
    let matches: Vec<&ScannedEntry> = entries
        .iter()
        .filter(|e| e.key.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [] => Err(KeyLookupError::NotFound),
        [one] => Ok(one),
        many => Err(KeyLookupError::Ambiguous(
            many.iter().map(|e| e.key.clone()).collect(),
        )),
    }
}

/// Removal outcome shared by evict, prune, and clear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Removed {
    pub entries: u64,
    pub bytes: u64,
}

/// Delete one scanned entry's file.
pub fn evict_entry(entry: &ScannedEntry) -> Removed {
    if fs::remove_file(&entry.path).is_ok() {
        Removed {
            entries: 1,
            bytes: entry.bytes,
        }
    } else {
        Removed::default()
    }
}

/// Delete entries older than `older_than` plus orphaned temp files,
/// across every level. The manual GC form behind `cache prune`.
#[must_use]
pub fn prune(caches: &WorkspaceCaches, older_than: Duration) -> Removed {
    let mut removed = Removed::default();
    for entry in scan(caches) {
        if entry.age > older_than {
            let r = evict_entry(&entry);
            removed.entries += r.entries;
            removed.bytes += r.bytes;
        }
    }
    for (dir, _) in caches.levels() {
        removed.entries += sweep_dir(dir, None);
    }
    removed
}

/// Delete every entry across every level. Non-entry files survive.
#[must_use]
pub fn clear(caches: &WorkspaceCaches) -> Removed {
    let mut removed = Removed::default();
    for entry in scan(caches) {
        let r = evict_entry(&entry);
        removed.entries += r.entries;
        removed.bytes += r.bytes;
    }
    for (dir, _) in caches.levels() {
        removed.entries += sweep_dir(dir, None);
    }
    removed
}
