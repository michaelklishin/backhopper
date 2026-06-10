// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The content-keyed verdict cache behind the `check` and `bisect`
//! verbs.
//!
//! Two levels: `by-input/` keys on the commit SHA plus every
//! evaluation input (the fast path), `by-content/` keys on the
//! normalized patch plus the macro environment — what the evaluation
//! actually consumes — so a `git cherry-pick -x` hop that re-mints
//! the SHA still hits. A content hit mints an input-level alias for
//! the new SHA inside `lookup_content` itself, so no caller can
//! forget it.
//!
//! The lookup and store protocol is type-state: `lookup` yields an
//! `InputMissToken`, `lookup_content` consumes it and yields a
//! `MissToken`, and `store` consumes that. Storing without both
//! lookups, storing twice, or storing under keys that were never
//! looked up does not compile.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_core::model::verdict::SeriesEvaluation;

use crate::cache_io::{content_hash, entry_file_name, is_older_than, write_atomic};
use crate::sweep::maybe_daily_sweep;

pub const VERDICT_CACHE_DIR_NAME: &str = ".verdict_cache";
pub const SIBLINGS_CACHE_DIR_NAME: &str = ".siblings_doctor_cache";
const BY_INPUT_DIR: &str = "by-input";
const BY_CONTENT_DIR: &str = "by-content";

/// On-disk verdict-entry layout version. Independent of the envelope
/// schema; a mismatch reads as miss-and-delete, so the format can
/// evolve without migration ceremony.
pub const VERDICT_ENTRY_FORMAT_VERSION: u32 = 1;

/// Which evaluation pipeline produced (and may consume) an entry.
/// `bisect` evaluates without macro context, pin files, or path
/// routing, so its entries must never serve a `check` lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationShape {
    Check,
    Bisect,
}

/// One pin's contribution to the key: the resolved tag commit (the
/// live repo is read there for pin files) and the snapshot file hash
/// (catches regenerations the `(project, tag)` pair cannot see).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinKeyRow {
    pub project: ProjectName,
    pub tag: TagName,
    /// `None` for shapes that never read the pin's repo (`bisect`
    /// and source-side pins).
    pub resolved_tag_sha: Option<CommitSha>,
    pub snapshot_blake3: String,
}

/// Target-repo inputs, when `--target-repo-dir-path` is in play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetKeyInputs {
    pub resolved_commit: CommitSha,
    pub translations_blake3: String,
}

/// The input-level (L1) key pre-image: the full 40-char SHA plus
/// every input the evaluation reads. Serialized to canonical JSON for
/// hashing and stored inside the entry as the debuggable pre-image —
/// one definition serves both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKeyInputs {
    pub shape: EvaluationShape,
    pub commit: CommitSha,
    /// The series the pins came from, when one was named. Joins the
    /// key (two series with identical pins split entries — accepted
    /// for inspectability) and powers `cache list --series`.
    pub series: Option<SeriesName>,
    pub pins: Vec<PinKeyRow>,
    pub source_pins: Vec<PinKeyRow>,
    pub target: Option<TargetKeyInputs>,
    pub config_blake3: String,
    pub schema_version: u32,
    pub crate_version: String,
}

impl CacheKeyInputs {
    /// Derive the content-level key: the SHA component is replaced by
    /// hashes of what the evaluation actually consumes, every other
    /// component carries over unchanged. The only way to build a
    /// `ContentKeyInputs`, so the two levels can never disagree on
    /// the shared components.
    #[must_use]
    pub fn content_key(
        &self,
        patch_blake3: String,
        macro_env_blake3: Option<String>,
    ) -> ContentKeyInputs {
        ContentKeyInputs {
            shape: self.shape,
            series: self.series.clone(),
            pins: self.pins.clone(),
            source_pins: self.source_pins.clone(),
            target: self.target.clone(),
            config_blake3: self.config_blake3.clone(),
            schema_version: self.schema_version,
            crate_version: self.crate_version.clone(),
            patch_blake3,
            macro_env_blake3,
        }
    }
}

/// The content-level (L2) key pre-image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentKeyInputs {
    pub shape: EvaluationShape,
    pub series: Option<SeriesName>,
    pub pins: Vec<PinKeyRow>,
    pub source_pins: Vec<PinKeyRow>,
    pub target: Option<TargetKeyInputs>,
    pub config_blake3: String,
    pub schema_version: u32,
    pub crate_version: String,
    /// Hash of the normalized patch: paths plus content lines, `@@`
    /// offset headers excluded, so pure offset drift hits.
    pub patch_blake3: String,
    /// Hash of the macro environment the touched files resolve to.
    /// `None` for shapes that consume no macros (`bisect`).
    pub macro_env_blake3: Option<String>,
}

/// L1 lookup result.
#[derive(Debug)]
pub enum InputOutcome<'c> {
    Hit(SeriesEvaluation),
    MissOnInput(InputMissToken<'c>),
}

/// L2 lookup result. On `Hit`, the input-level alias for the new SHA
/// was minted before returning.
#[derive(Debug)]
pub enum ContentOutcome<'c> {
    Hit(SeriesEvaluation),
    Miss(MissToken<'c>),
}

/// Proof of an L1 miss. Consumed by the L2 lookup.
#[derive(Debug)]
#[must_use = "consume the token via lookup_content, or drop it to skip caching"]
pub struct InputMissToken<'c> {
    cache: &'c VerdictCache,
    input_hash: String,
    input_pre_image: Value,
}

/// Proof of misses on both levels. Consumed by `store`.
#[derive(Debug)]
#[must_use = "consume the token via store, or drop it to skip caching"]
pub struct MissToken<'c> {
    cache: &'c VerdictCache,
    input_hash: String,
    input_pre_image: Value,
    content_hash: String,
    content_pre_image: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct VerdictEntry {
    entry_format_version: u32,
    created_at: String,
    key_inputs: Value,
    /// `Some` for input-level entries minted from a content-level
    /// hit; records which content key they matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alias_of_content_key: Option<String>,
    evaluation: SeriesEvaluation,
}

/// The two-level verdict cache under one snapshot workspace.
#[derive(Debug)]
pub struct VerdictCache {
    by_input: PathBuf,
    by_content: PathBuf,
    siblings_dir: PathBuf,
    sweep_marker: PathBuf,
    ttl: Option<Duration>,
    swept: OnceLock<()>,
}

impl VerdictCache {
    /// Open the workspace's verdict cache. `ttl_days = 0` disables
    /// expiry.
    pub fn open(snapshot_dir: &Path, ttl_days: u32) -> Self {
        let root = snapshot_dir.join(VERDICT_CACHE_DIR_NAME);
        Self {
            by_input: root.join(BY_INPUT_DIR),
            by_content: root.join(BY_CONTENT_DIR),
            siblings_dir: snapshot_dir.join(SIBLINGS_CACHE_DIR_NAME),
            sweep_marker: root.join(".last_sweep"),
            ttl: ttl_from_days(ttl_days),
            swept: OnceLock::new(),
        }
    }

    /// L1: SHA-keyed, consulted before patch extraction.
    pub fn lookup(&self, inputs: &CacheKeyInputs) -> InputOutcome<'_> {
        let Ok(hash) = content_hash(inputs) else {
            // unserialisable inputs cannot be keyed; treat as a miss
            // that will also fail to store
            return InputOutcome::MissOnInput(InputMissToken {
                cache: self,
                input_hash: String::new(),
                input_pre_image: Value::Null,
            });
        };
        let path = self.by_input.join(entry_file_name(&hash));
        if let Some(entry) = self.read_entry(&path) {
            return InputOutcome::Hit(entry.evaluation);
        }
        InputOutcome::MissOnInput(InputMissToken {
            cache: self,
            input_hash: hash,
            input_pre_image: serde_json::to_value(inputs).unwrap_or(Value::Null),
        })
    }

    fn read_entry(&self, path: &Path) -> Option<VerdictEntry> {
        if let Some(ttl) = self.ttl
            && is_older_than(path, ttl)
        {
            let _ = fs::remove_file(path);
            return None;
        }
        let bytes = fs::read(path).ok()?;
        let Ok(entry) = serde_json::from_slice::<VerdictEntry>(&bytes) else {
            // torn or foreign layout: reconstructible, so delete
            let _ = fs::remove_file(path);
            return None;
        };
        if entry.entry_format_version != VERDICT_ENTRY_FORMAT_VERSION {
            let _ = fs::remove_file(path);
            return None;
        }
        Some(entry)
    }

    fn write_entry(
        &self,
        dir: &Path,
        hash: &str,
        key_inputs: Value,
        alias_of_content_key: Option<String>,
        evaluation: &SeriesEvaluation,
    ) {
        self.sweep_once();
        if hash.is_empty() {
            return;
        }
        let entry = VerdictEntry {
            entry_format_version: VERDICT_ENTRY_FORMAT_VERSION,
            created_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default(),
            key_inputs,
            alias_of_content_key,
            evaluation: evaluation.clone(),
        };
        let result = fs::create_dir_all(dir)
            .map_err(Into::into)
            .and_then(|()| serde_json::to_vec_pretty(&entry).map_err(Into::into))
            .and_then(|bytes| write_atomic(&dir.join(entry_file_name(hash)), &bytes));
        if let Err(e) = result {
            tracing::warn!("failed to write a verdict cache entry: {e}");
        }
    }

    // the once-per-day expired-entry sweep, amortized into the first
    // cache write of a run
    fn sweep_once(&self) {
        self.swept.get_or_init(|| {
            maybe_daily_sweep(
                &[
                    self.by_input.clone(),
                    self.by_content.clone(),
                    self.siblings_dir.clone(),
                ],
                self.ttl,
                &self.sweep_marker,
            );
        });
    }
}

impl<'c> InputMissToken<'c> {
    /// L2: content-keyed, consulted after patch extraction. On a hit,
    /// the L1 alias for the looked-up SHA is minted here, inside the
    /// lookup, so no caller can forget it.
    pub fn lookup_content(self, content: &ContentKeyInputs) -> ContentOutcome<'c> {
        let Ok(hash) = content_hash(content) else {
            return ContentOutcome::Miss(MissToken {
                cache: self.cache,
                input_hash: self.input_hash,
                input_pre_image: self.input_pre_image,
                content_hash: String::new(),
                content_pre_image: Value::Null,
            });
        };
        let path = self.cache.by_content.join(entry_file_name(&hash));
        if let Some(entry) = self.cache.read_entry(&path) {
            self.cache.write_entry(
                &self.cache.by_input,
                &self.input_hash,
                self.input_pre_image,
                Some(hash),
                &entry.evaluation,
            );
            return ContentOutcome::Hit(entry.evaluation);
        }
        ContentOutcome::Miss(MissToken {
            cache: self.cache,
            input_hash: self.input_hash,
            input_pre_image: self.input_pre_image,
            content_hash: hash,
            content_pre_image: serde_json::to_value(content).unwrap_or(Value::Null),
        })
    }
}

impl MissToken<'_> {
    /// Consume the token: store the evaluation once, under both the
    /// input and content keys that were looked up on the way here.
    pub fn store(self, evaluation: &SeriesEvaluation) {
        self.cache.write_entry(
            &self.cache.by_content,
            &self.content_hash,
            self.content_pre_image,
            None,
            evaluation,
        );
        self.cache.write_entry(
            &self.cache.by_input,
            &self.input_hash,
            self.input_pre_image,
            None,
            evaluation,
        );
    }
}

/// `ttl_days = 0` means no expiry.
#[must_use]
pub fn ttl_from_days(ttl_days: u32) -> Option<Duration> {
    if ttl_days == 0 {
        None
    } else {
        Some(Duration::from_secs(u64::from(ttl_days) * 24 * 60 * 60))
    }
}
