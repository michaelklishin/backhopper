// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! On-disk cache code shared by backhopper's cached analyses.
//!
//! `cache_io` provides content-addressed entries: the key serialises
//! to canonical JSON, its BLAKE3 hash names the entry file, and a
//! separate freshness document decides whether a stored value may be
//! served. `policy` resolves whether caching is on at all for this
//! process.

pub mod cache_io;
pub mod errors;
pub mod inspect;
pub mod policy;
pub mod sweep;
pub mod verdict;

pub use cache_io::{
    ENTRY_FORMAT_VERSION, canonical_json, content_hash, hash_bytes, hash_file, is_entry_file_name,
};
pub use errors::CacheError;
pub use inspect::{
    KeyLookupError, Removed, ScannedEntry, WorkspaceCaches, clear, evict_entry, find_by_key_prefix,
    prune, scan, stats,
};
pub use policy::{CacheMode, FORCE_CACHE_ENV, NO_CACHE_ENV};
pub use sweep::{maybe_daily_sweep, sweep_dir};
pub use verdict::{
    CacheKeyInputs, CachedVerdict, ContentKeyInputs, ContentOutcome, EvaluationShape,
    InputMissToken, InputOutcome, MissToken, PinKeyRow, TargetKeyInputs, VERDICT_CACHE_DIR_NAME,
    VerdictCache, ttl_from_days,
};
