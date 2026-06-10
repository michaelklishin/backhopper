// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::Subcommand;

use backhopper_core::model::names::{CommitShaPrefix, SeriesName};

#[derive(Debug, Subcommand)]
pub enum CacheCmd {
    /// Entry counts per level, alias count, total bytes, and the
    /// write-time range, across both workspace caches.
    Stats,
    /// One row per cache entry; scan-and-filter over the entry
    /// pre-images.
    List {
        /// Keep entries whose pre-image commit starts with this SHA
        /// prefix.
        #[arg(long)]
        commit: Option<CommitShaPrefix>,
        /// Keep entries whose pre-image names this series.
        #[arg(long)]
        series: Option<SeriesName>,
        /// Keep entries at least this many days old.
        #[arg(long)]
        min_age_days: Option<u32>,
    },
    /// Print one entry: the key pre-image, level, age, size, alias
    /// breadcrumb, and the verdict summary.
    Show {
        /// Unique cache-key prefix, 8 or more hex characters; find
        /// one with `cache list`.
        #[arg(value_name = "KEY_PREFIX")]
        key_prefix: String,
        /// Include the full stored value.
        #[arg(long)]
        full: bool,
    },
    /// Delete specific entries by key prefix, or every entry whose
    /// pre-image references a commit.
    Evict {
        /// Unique cache-key prefixes, 8 or more hex characters each.
        #[arg(value_name = "KEY_PREFIX", required_unless_present = "commit")]
        key_prefixes: Vec<String>,
        /// Evict every entry whose pre-image references this commit.
        #[arg(long, conflicts_with = "key_prefixes")]
        commit: Option<CommitShaPrefix>,
    },
    /// Delete entries older than N days plus orphaned temp files,
    /// overriding the configured `ttl_days` for one sweep.
    Prune {
        #[arg(long, value_name = "DAYS")]
        older_than: u32,
    },
    /// Delete every cache entry. Non-entry files survive.
    Clear,
}
