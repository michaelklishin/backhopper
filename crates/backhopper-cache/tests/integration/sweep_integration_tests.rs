// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The sweep deletes files; these pin what it may and may not touch.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use backhopper_cache::{maybe_daily_sweep, sweep_dir};

fn write_aged(dir: &Path, name: &str, age: Duration) {
    let path = dir.join(name);
    fs::write(&path, b"x").unwrap();
    let f = fs::File::options().write(true).open(&path).unwrap();
    f.set_modified(SystemTime::now() - age).unwrap();
}

const TWO_DAYS: Duration = Duration::from_secs(2 * 24 * 60 * 60);
const ONE_DAY: Duration = Duration::from_secs(24 * 60 * 60);
const TWO_HOURS: Duration = Duration::from_secs(2 * 60 * 60);

#[test]
fn sweep_removes_only_expired_entries_and_orphaned_tmps() {
    let dir = TempDir::new().unwrap();
    write_aged(dir.path(), "v11-0123456789abcdef.json", TWO_DAYS);
    write_aged(dir.path(), "v11-fedcba9876543210.json", Duration::ZERO);
    write_aged(dir.path(), "leftover.tmp", TWO_HOURS);
    write_aged(dir.path(), "fresh.tmp", Duration::ZERO);
    // entry-shaped names only: a stray file must never be deleted
    write_aged(dir.path(), "README.md", TWO_DAYS);

    let removed = sweep_dir(dir.path(), Some(ONE_DAY));

    assert_eq!(removed, 2);
    assert!(!dir.path().join("v11-0123456789abcdef.json").exists());
    assert!(dir.path().join("v11-fedcba9876543210.json").exists());
    assert!(!dir.path().join("leftover.tmp").exists());
    assert!(dir.path().join("fresh.tmp").exists());
    assert!(dir.path().join("README.md").exists());
}

#[test]
fn sweep_without_ttl_keeps_entries_and_still_prunes_orphans() {
    let dir = TempDir::new().unwrap();
    write_aged(dir.path(), "v11-0123456789abcdef.json", TWO_DAYS);
    write_aged(dir.path(), "leftover.tmp", TWO_HOURS);

    let removed = sweep_dir(dir.path(), None);

    assert_eq!(removed, 1);
    assert!(dir.path().join("v11-0123456789abcdef.json").exists());
    assert!(!dir.path().join("leftover.tmp").exists());
}

#[test]
fn sweep_of_a_missing_dir_removes_nothing() {
    let dir = TempDir::new().unwrap();
    let gone = dir.path().join("never-created");
    assert_eq!(sweep_dir(&gone, Some(ONE_DAY)), 0);
}

#[test]
fn daily_sweep_runs_once_then_defers_to_the_marker() {
    let dir = TempDir::new().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    let marker = dir.path().join("markers").join("swept");
    write_aged(&cache, "leftover.tmp", TWO_HOURS);

    maybe_daily_sweep(&[cache.clone()], None, &marker);
    assert!(marker.exists(), "first run writes the marker");
    assert!(!cache.join("leftover.tmp").exists());

    write_aged(&cache, "second.tmp", TWO_HOURS);
    maybe_daily_sweep(&[cache.clone()], None, &marker);
    assert!(
        cache.join("second.tmp").exists(),
        "a fresh marker defers the next sweep"
    );
}
