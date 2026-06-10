// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! On-disk cache plumbing shared by backhopper's cached analyses.
//!
//! `cache_io` provides content-addressed entries: the key serialises
//! to canonical JSON, its BLAKE3 hash names the entry file, and a
//! separate freshness document decides whether a stored value may be
//! served. `policy` resolves whether caching is on at all for this
//! process.

pub mod cache_io;
pub mod errors;
pub mod policy;

pub use cache_io::{CacheDir, ENTRY_FORMAT_VERSION, canonical_json, content_hash};
pub use errors::CacheError;
pub use policy::{CacheMode, FORCE_CACHE_ENV, NO_CACHE_ENV};
