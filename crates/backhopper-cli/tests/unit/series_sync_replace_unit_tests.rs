// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::commands::series::{PinPayload, SyncOutput, replace_series_block};

const STARTING_CONFIG: &str = r#"# top-of-file comment
config_version = 1

[defaults]
snapshot_dir = "/path/to/snapshots"

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[series]]
name = "rabbitmq-4.1"
pins = [
    { project = "ra", tag = "v2.16.0" },
]

[[series]]
name = "rabbitmq-4.2"
pins = [
    { project = "ra", tag = "v2.17.0" },
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
fn replace_replaces_existing_series_in_place() {
    let out = replace_series_block(
        STARTING_CONFIG,
        &payload("rabbitmq-4.1", &[("ra", "v2.16.13")]),
    )
    .unwrap();
    assert!(out.contains("v2.16.13"), "{out}");
    assert!(!out.contains("v2.16.0"), "old pin gone: {out}");
    assert!(out.contains("v2.17.0"), "unrelated series untouched: {out}");
}

#[test]
fn replace_preserves_top_of_file_comment() {
    let out = replace_series_block(
        STARTING_CONFIG,
        &payload("rabbitmq-4.1", &[("ra", "v2.16.13")]),
    )
    .unwrap();
    assert!(out.contains("# top-of-file comment"), "{out}");
}

#[test]
fn replace_appends_new_series_when_absent() {
    let out = replace_series_block(
        STARTING_CONFIG,
        &payload("rabbitmq-3.13", &[("ra", "v2.15.3")]),
    )
    .unwrap();
    assert!(out.contains("rabbitmq-3.13"));
    assert!(out.contains("v2.15.3"));
    assert!(out.contains("rabbitmq-4.1"), "existing series kept");
    assert!(out.contains("rabbitmq-4.2"), "existing series kept");
}

#[test]
fn replace_preserves_unrelated_projects_block() {
    let out = replace_series_block(
        STARTING_CONFIG,
        &payload("rabbitmq-4.1", &[("ra", "v2.16.13")]),
    )
    .unwrap();
    assert!(out.contains("[[project]]"));
    assert!(out.contains("/tmp/ra.git"));
}

#[test]
fn replace_preserves_defaults_block() {
    let out = replace_series_block(
        STARTING_CONFIG,
        &payload("rabbitmq-4.1", &[("ra", "v2.16.13")]),
    )
    .unwrap();
    assert!(out.contains("[defaults]"));
    assert!(out.contains("/path/to/snapshots"));
}

#[test]
fn replace_is_idempotent_when_applied_twice_with_same_payload() {
    let p = payload("rabbitmq-4.1", &[("ra", "v2.16.13")]);
    let once = replace_series_block(STARTING_CONFIG, &p).unwrap();
    let twice = replace_series_block(&once, &p).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn replace_rejects_invalid_toml() {
    let r = replace_series_block(
        "this is not valid toml ###",
        &payload("x", &[("ra", "v1.0.0")]),
    );
    assert!(r.is_err());
}

#[test]
fn replace_works_on_config_with_no_existing_series_array() {
    let bare = r#"config_version = 1

[defaults]
snapshot_dir = "/tmp"

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"
"#;
    let out = replace_series_block(bare, &payload("rabbitmq-4.1", &[("ra", "v2.16.13")])).unwrap();
    assert!(out.contains("[[series]]"), "{out}");
    assert!(out.contains("rabbitmq-4.1"));
}

#[test]
fn replace_renders_inline_table_for_each_pin() {
    let out = replace_series_block(
        STARTING_CONFIG,
        &payload("rabbitmq-4.1", &[("ra", "v2.16.13"), ("osiris", "v1.8.8")]),
    )
    .unwrap();
    assert!(out.contains("project = \"ra\""), "{out}");
    assert!(out.contains("tag = \"v2.16.13\""), "{out}");
    assert!(out.contains("project = \"osiris\""), "{out}");
    assert!(out.contains("tag = \"v1.8.8\""), "{out}");
}
