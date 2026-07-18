// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{ApplicationName, CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
use backhopper_core::snapshot::{format, parser};

fn header_with_apps(apps: &[&str]) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("otp").unwrap(),
        tag: TagName::new("OTP-26.2.5").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["lib/*".into(), "erts/preloaded".into()],
        apps_scanned: apps
            .iter()
            .map(|s| ApplicationName::new(*s).unwrap())
            .collect(),
        generated_by: "backhopper".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

#[test]
fn apps_scanned_header_round_trips_through_writer_and_parser() {
    let header = header_with_apps(&["kernel", "stdlib"]);
    let snap = Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(text.contains("# apps-scanned: kernel, stdlib"));
    let parsed = parser::parse(&text).unwrap();
    let names: Vec<&str> = parsed
        .header()
        .apps_scanned
        .iter()
        .map(|a| a.as_str())
        .collect();
    assert_eq!(names, vec!["kernel", "stdlib"]);
}

#[test]
fn empty_apps_scanned_emits_no_header_line() {
    let header = header_with_apps(&[]);
    let snap = Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(!text.contains("apps-scanned"));
    let parsed = parser::parse(&text).unwrap();
    assert!(parsed.header().apps_scanned.is_empty());
}

#[test]
fn parser_accepts_pre_apps_scanned_snapshots_as_empty() {
    // Snapshots written before apps-scanned lack the line: parse as an empty list, not an error.
    let legacy = "\
# backhopper snapshot
# format-version: 1
# project: ra
# tag: v1.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src/**/*.erl
# generated-by: backhopper 0.4.0
# generated-at: 2026-01-01T00:00:00Z
";
    let parsed = parser::parse(legacy).unwrap();
    assert!(parsed.header().apps_scanned.is_empty());
}

#[test]
fn into_canonical_sorts_and_dedups_apps_scanned() {
    let header = header_with_apps(&["stdlib", "kernel", "stdlib"]);
    let snap = Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical();
    let names: Vec<&str> = snap
        .header()
        .apps_scanned
        .iter()
        .map(|a| a.as_str())
        .collect();
    assert_eq!(names, vec!["kernel", "stdlib"]);
}
