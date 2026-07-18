// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The opportunistic once-per-day cache sweep.
//!
//! Read-time expiry cleans entries that get looked up; entries that
//! are never looked up again need a separate cleanup. On the first cache write of a run, if the
//! marker file is older than a day, expired entries and orphaned temp
//! files are swept across every workspace cache directory, then the
//! marker is touched. At most one sweep per day per workspace,
//! folded into a run that is already doing an evaluation.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cache_io::{is_entry_file_name, is_older_than};

const SWEEP_INTERVAL: Duration = Duration::from_hours(24);

/// Temp files this old can only be leftovers from a crashed writer;
/// live writers hold theirs for milliseconds.
const ORPHAN_TMP_AGE: Duration = Duration::from_hours(1);

/// Sweep `dirs` if the marker is stale, then touch it. `ttl = None`
/// still sweeps orphaned temp files, just not entries.
pub fn maybe_daily_sweep(dirs: &[PathBuf], ttl: Option<Duration>, marker: &Path) {
    if !is_older_than(marker, SWEEP_INTERVAL) && marker.exists() {
        return;
    }
    for dir in dirs {
        sweep_dir(dir, ttl);
    }
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(marker, b"");
}

/// Delete expired entry-shaped files and orphaned temp files in
/// `dir`. Only names matching the entry shape (or `.tmp` leftovers)
/// are ever deleted, so a misconfigured directory cannot lose real
/// data.
pub fn sweep_dir(dir: &Path, ttl: Option<Duration>) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let expired_entry =
            is_entry_file_name(name) && ttl.is_some_and(|max_age| is_older_than(&path, max_age));
        let orphaned_tmp = name.ends_with(".tmp") && is_older_than(&path, ORPHAN_TMP_AGE);
        if (expired_entry || orphaned_tmp) && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}
