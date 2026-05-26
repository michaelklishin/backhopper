// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::commands::series::{
    MergeConflict, PinPayload, SyncOutput, merge_sync_into_config_text,
};
use backhopper_core::model::names::{ProjectName, TagName};

const STARTING_CONFIG: &str = r#"# top-of-file comment
config_version = 1

[defaults]
snapshot_dir = "/path/to/snapshots"

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[series]]
name = "rabbitmq-4.2"
pins = [
    { project = "ra", tag = "v2.16.0" },
    { project = "khepri", tag = "v0.15.0" },
]
"#;

fn payload(name: &str, pins: &[(&str, &str)]) -> SyncOutput {
    SyncOutput {
        name: name.into(),
        branch: None,
        pins: pins
            .iter()
            .map(|(p, t)| PinPayload {
                project: (*p).into(),
                tag: (*t).into(),
            })
            .collect(),
        dropped_unconfigured: Vec::new(),
        skipped: Vec::new(),
    }
}

#[test]
fn merge_adds_new_pins_without_touching_existing() {
    let (out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("seshat", "v1.0.1")]),
        false,
    )
    .unwrap();
    assert!(out.contains("v2.16.0"), "existing ra pin kept: {out}");
    assert!(out.contains("v0.15.0"), "existing khepri pin kept: {out}");
    assert!(out.contains("seshat"), "new pin added: {out}");
    assert!(out.contains("v1.0.1"));
    assert_eq!(outcome.added.len(), 1);
    assert_eq!(outcome.added[0].project, "seshat");
    assert_eq!(outcome.preserved.len(), 2);
    assert!(outcome.skipped_conflicts.is_empty());
}

#[test]
fn merge_never_drops_pins_absent_from_inferred_set() {
    let (out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("ra", "v2.16.0")]),
        false,
    )
    .unwrap();
    assert!(
        out.contains("v0.15.0"),
        "khepri pin must not be dropped just because inferred set omits it: {out}"
    );
    let preserved_names: Vec<_> = outcome.preserved.iter().map(|p| &p.project).collect();
    assert_eq!(preserved_names, vec!["khepri"]);
}

#[test]
fn merge_skips_conflicting_pins_and_reports_them() {
    let (out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("ra", "v2.16.7")]),
        false,
    )
    .unwrap();
    assert!(
        out.contains("v2.16.0"),
        "existing tag kept by default: {out}"
    );
    assert!(
        !out.contains("v2.16.7"),
        "new tag must not be written: {out}"
    );
    assert_eq!(
        outcome.skipped_conflicts,
        vec![MergeConflict {
            project: ProjectName::new("ra").unwrap(),
            existing_tag: TagName::new("v2.16.0").unwrap(),
            inferred_tag: TagName::new("v2.16.7").unwrap(),
        }]
    );
    assert!(outcome.updated.is_empty());
}

#[test]
fn merge_with_overwrite_existing_applies_conflicts() {
    let (out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("ra", "v2.16.7")]),
        true,
    )
    .unwrap();
    assert!(out.contains("v2.16.7"));
    assert!(!out.contains("v2.16.0"));
    assert_eq!(outcome.updated.len(), 1);
    assert_eq!(outcome.updated[0].project.as_str(), "ra");
    assert_eq!(outcome.updated[0].existing_tag.as_str(), "v2.16.0");
    assert_eq!(outcome.updated[0].inferred_tag.as_str(), "v2.16.7");
    assert!(outcome.skipped_conflicts.is_empty());
}

#[test]
fn merge_unchanged_pin_is_a_noop() {
    let (out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("ra", "v2.16.0")]),
        false,
    )
    .unwrap();
    assert!(out.contains("v2.16.0"));
    assert_eq!(outcome.unchanged.len(), 1);
    assert!(outcome.added.is_empty());
    assert!(outcome.updated.is_empty());
    assert!(outcome.skipped_conflicts.is_empty());
}

#[test]
fn merge_creates_series_when_absent() {
    let (out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.3", &[("ra", "v2.17.0")]),
        false,
    )
    .unwrap();
    assert!(out.contains("rabbitmq-4.3"));
    assert!(out.contains("v2.17.0"));
    assert!(out.contains("rabbitmq-4.2"), "old series untouched");
    assert!(out.contains("v0.15.0"), "old khepri pin untouched");
    assert_eq!(outcome.added.len(), 1);
    assert!(outcome.preserved.is_empty());
}

#[test]
fn merge_preserves_top_of_file_comment_and_unrelated_blocks() {
    let (out, _) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("seshat", "v1.0.1")]),
        false,
    )
    .unwrap();
    assert!(out.contains("# top-of-file comment"));
    assert!(out.contains("[defaults]"));
    assert!(out.contains("/path/to/snapshots"));
    assert!(out.contains("/tmp/ra.git"));
}

#[test]
fn merge_is_idempotent_for_pure_addition() {
    let p = payload("rabbitmq-4.2", &[("seshat", "v1.0.1")]);
    let (once, _) = merge_sync_into_config_text(STARTING_CONFIG, &p, false).unwrap();
    let (twice, outcome) = merge_sync_into_config_text(&once, &p, false).unwrap();
    assert_eq!(once, twice);
    assert!(outcome.added.is_empty());
    assert_eq!(outcome.unchanged.len(), 1);
}

#[test]
fn merge_mixed_add_update_unchanged_conflict_preserve() {
    let (_out, outcome) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload(
            "rabbitmq-4.2",
            &[
                ("ra", "v2.16.7"),
                ("khepri", "v0.15.0"),
                ("seshat", "v1.0.1"),
            ],
        ),
        false,
    )
    .unwrap();
    assert_eq!(outcome.added.len(), 1, "seshat added");
    assert_eq!(outcome.unchanged.len(), 1, "khepri unchanged");
    assert_eq!(outcome.skipped_conflicts.len(), 1, "ra conflicts");
    assert!(outcome.preserved.is_empty(), "all existing covered");
}

#[test]
fn merge_does_not_duplicate_a_project_that_is_pattern_pinned() {
    let with_pattern = r#"config_version = 1

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[series]]
name = "rabbitmq-4.2"
pins = [
    { project = "ra", tag_pattern = "v2.*", select = "latest" },
]
"#;
    let (out, outcome) = merge_sync_into_config_text(
        with_pattern,
        &payload("rabbitmq-4.2", &[("ra", "v2.16.7")]),
        false,
    )
    .unwrap();
    let ra_count = out.matches("project = \"ra\"").count();
    assert_eq!(
        ra_count, 1,
        "ra must appear once, not duplicated as literal: {out}"
    );
    assert!(
        outcome.added.is_empty(),
        "non-literal existing pin must block additive add: {outcome:?}"
    );
}

#[test]
fn merge_rejects_pins_that_is_not_an_array() {
    let broken = r#"config_version = 1

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[series]]
name = "rabbitmq-4.2"
pins = "broken"
"#;
    let r = merge_sync_into_config_text(
        broken,
        &payload("rabbitmq-4.2", &[("ra", "v2.16.0")]),
        false,
    );
    assert!(r.is_err());
}

#[test]
fn merge_rejects_invalid_toml() {
    let r = merge_sync_into_config_text(
        "this is not valid toml ###",
        &payload("x", &[("ra", "v1.0.0")]),
        false,
    );
    assert!(r.is_err());
}

#[test]
fn merge_adds_first_pin_to_empty_pins_array() {
    let bare = r#"config_version = 1

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[series]]
name = "rabbitmq-4.2"
pins = []
"#;
    let (out, outcome) =
        merge_sync_into_config_text(bare, &payload("rabbitmq-4.2", &[("ra", "v2.16.0")]), false)
            .unwrap();
    assert_eq!(outcome.added.len(), 1);
    assert!(outcome.preserved.is_empty());
    assert!(out.contains("v2.16.0"));
}

#[test]
fn merge_writes_to_empty_config_without_series_array() {
    let bare = r#"config_version = 1

[defaults]
snapshot_dir = "/tmp"

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"
"#;
    let (out, outcome) =
        merge_sync_into_config_text(bare, &payload("rabbitmq-4.2", &[("ra", "v2.16.0")]), false)
            .unwrap();
    assert!(out.contains("[[series]]"));
    assert!(out.contains("rabbitmq-4.2"));
    assert!(out.contains("v2.16.0"));
    assert_eq!(outcome.added.len(), 1);
    assert!(outcome.preserved.is_empty());
}
