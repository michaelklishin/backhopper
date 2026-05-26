// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::commands::series::{
    PinPayload, SyncOutput, merge_sync_into_config_text, replace_series_block, unified_diff,
};

const STARTING_CONFIG: &str = r#"config_version = 1

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[series]]
name = "rabbitmq-4.2"
pins = [
    { project = "ra", tag = "v2.16.0" },
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
fn unified_diff_of_identical_text_is_empty() {
    assert!(unified_diff("a\nb\n", "a\nb\n", "x.toml").is_empty());
}

#[test]
fn unified_diff_emits_file_headers_and_hunk() {
    let d = unified_diff("a\nb\n", "a\nC\n", "config.toml");
    assert!(d.starts_with("--- a/config.toml\n"), "{d}");
    assert!(d.contains("+++ b/config.toml\n"), "{d}");
    assert!(d.contains("-b\n"), "{d}");
    assert!(d.contains("+C\n"), "{d}");
}

#[test]
fn diff_against_merge_addition_shows_only_added_pin() {
    let (after, _) = merge_sync_into_config_text(
        STARTING_CONFIG,
        &payload("rabbitmq-4.2", &[("seshat", "v1.0.1")]),
        false,
    )
    .unwrap();
    let d = unified_diff(STARTING_CONFIG, &after, "backhopper.toml");
    let added: Vec<_> = d
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .collect();
    let removed: Vec<_> = d
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .collect();
    assert!(added.iter().any(|l| l.contains("seshat")), "diff: {d}");
    assert!(added.iter().any(|l| l.contains("v1.0.1")), "diff: {d}");
    assert!(
        !removed.iter().any(|l| l.contains("\"ra\"")),
        "ra not removed: {d}"
    );
}

#[test]
fn diff_against_replace_shows_drop_of_unrelated_existing_pin() {
    let p = payload("rabbitmq-4.2", &[("seshat", "v1.0.1")]);
    let after_replace = replace_series_block(STARTING_CONFIG, &p).unwrap();
    let d_replace = unified_diff(STARTING_CONFIG, &after_replace, "backhopper.toml");
    let removed_replace: Vec<_> = d_replace
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .collect();
    assert!(
        removed_replace.iter().any(|l| l.contains("v2.16.0")),
        "replace removes the existing ra pin: {d_replace}"
    );

    let (after_merge, _) = merge_sync_into_config_text(STARTING_CONFIG, &p, false).unwrap();
    let d_merge = unified_diff(STARTING_CONFIG, &after_merge, "backhopper.toml");
    let removed_merge: Vec<_> = d_merge
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .collect();
    assert!(
        !removed_merge.iter().any(|l| l.contains("\"ra\"")),
        "merge keeps the ra pin: {d_merge}",
    );
}

#[test]
fn diff_against_merge_conflict_without_overwrite_is_empty() {
    let p = payload("rabbitmq-4.2", &[("ra", "v2.16.7")]);
    let (after, _) = merge_sync_into_config_text(STARTING_CONFIG, &p, false).unwrap();
    let d = unified_diff(STARTING_CONFIG, &after, "backhopper.toml");
    assert!(
        d.is_empty(),
        "merge skips conflicts so config text is unchanged: {d}"
    );
}

#[test]
fn diff_against_merge_conflict_with_overwrite_shows_tag_change() {
    let p = payload("rabbitmq-4.2", &[("ra", "v2.16.7")]);
    let (after, _) = merge_sync_into_config_text(STARTING_CONFIG, &p, true).unwrap();
    let d = unified_diff(STARTING_CONFIG, &after, "backhopper.toml");
    assert!(d.contains("v2.16.7"), "{d}");
    assert!(d.contains("v2.16.0"), "{d}");
}
