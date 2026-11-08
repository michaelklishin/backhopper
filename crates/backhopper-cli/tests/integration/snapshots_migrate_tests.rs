// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;

use predicates::prelude::*;
use tempfile::TempDir;

use backhopper_test_support::toml_path;

use crate::helpers::cli::run;

const FORMAT_4_RA: &str = "\
# backhopper snapshot
# format-version: 4
# project: ra
# tag: v2.17.1
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: backhopper 0.36.0
# generated-at: 2026-09-01T00:00:00Z
# extractor-version: 8

module ra
  path src/ra.erl
  export start/1
";

fn store_with_a_format_4_snapshot() -> (TempDir, std::path::PathBuf) {
    let workdir = TempDir::new().unwrap();
    let snapshots = workdir.path().join("snapshots");
    fs::create_dir_all(snapshots.join("ra")).unwrap();
    fs::write(snapshots.join("ra/v2.17.1.api.txt"), FORMAT_4_RA).unwrap();
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(
        &cfg,
        format!(
            "config_version = 1\n\n[defaults]\nsnapshot_dir = \"{}\"\n\n[[project]]\nname = \"ra\"\ngit_url = \"{}\"\n",
            toml_path(&snapshots),
            toml_path(Path::new("/nonexistent/ra.git")),
        ),
    )
    .unwrap();
    (workdir, cfg)
}

fn migrate(cfg: &Path) -> assert_cmd::assert::Assert {
    run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "migrate",
    ])
}

#[test]
fn migrate_leaves_a_format_4_store_at_format_4() {
    let (workdir, cfg) = store_with_a_format_4_snapshot();
    migrate(&cfg);
    let after = fs::read_to_string(workdir.path().join("snapshots/ra/v2.17.1.api.txt")).unwrap();
    assert_eq!(after, FORMAT_4_RA);
}

#[test]
fn migrate_names_the_refresh_command_as_the_way_to_format_5() {
    let (_workdir, cfg) = store_with_a_format_4_snapshot();
    migrate(&cfg)
        .failure()
        .stdout(predicate::str::contains("--refresh-stale"))
        .stdout(predicate::str::contains("format 4"));
}

#[test]
fn migrate_does_not_report_a_format_5_snapshot_as_failed() {
    let (workdir, cfg) = store_with_a_format_4_snapshot();
    let path = workdir.path().join("snapshots/ra/v2.17.1.api.txt");
    let format_5 = FORMAT_4_RA
        .replace("format-version: 4", "format-version: 5")
        .replace("  path src/ra.erl\n", "  path src/ra.erl\n  app ra\n");
    fs::write(&path, &format_5).unwrap();
    migrate(&cfg).success();
    assert_eq!(fs::read_to_string(&path).unwrap(), format_5);
}
