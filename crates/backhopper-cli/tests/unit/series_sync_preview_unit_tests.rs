// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::cli::series::{DEFAULT_BRANCHES, default_branches};
use backhopper_cli::commands::rabbitmq_components::series_name_for_branch;
use backhopper_cli::commands::series::{
    PinPayload, PreviewOutput, SkippedPin, SyncOutput, render_preview_text,
};

fn stanza(name: &str, branch: &str, pin: (&str, &str)) -> SyncOutput {
    SyncOutput {
        name: name.into(),
        branch: Some(branch.into()),
        pins: vec![PinPayload {
            project: pin.0.into(),
            tag: pin.1.into(),
        }],
        dropped_unconfigured: Vec::new(),
        skipped: Vec::new(),
    }
}

fn render(output: &PreviewOutput) -> String {
    let mut buf = Vec::new();
    render_preview_text(output, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn preview_text_with_no_series_writes_nothing() {
    let output = PreviewOutput {
        series: Vec::new(),
        show_skipped: false,
    };
    assert!(render(&output).is_empty());
}

#[test]
fn preview_text_renders_single_stanza_without_trailing_separator() {
    let output = PreviewOutput {
        series: vec![stanza("rabbitmq-4.2", "v4.2.x", ("ra", "v2.17.3"))],
        show_skipped: false,
    };
    let s = render(&output);
    assert!(s.contains("[[series]]"));
    assert!(s.contains("name = \"rabbitmq-4.2\""));
    assert!(s.contains("# inferred from v4.2.x"));
    assert!(!s.ends_with("\n\n"), "no trailing blank line: {s:?}");
}

#[test]
fn preview_text_separates_multiple_stanzas_with_a_blank_line() {
    let output = PreviewOutput {
        series: vec![
            stanza("rabbitmq-main", "main", ("ra", "v3.0.0")),
            stanza("rabbitmq-4.2", "v4.2.x", ("ra", "v2.17.3")),
        ],
        show_skipped: false,
    };
    let s = render(&output);
    let header_indices: Vec<_> = s.match_indices("[[series]]").map(|(i, _)| i).collect();
    assert_eq!(header_indices.len(), 2, "two stanzas: {s}");
    assert!(s.contains("rabbitmq-main"));
    assert!(s.contains("rabbitmq-4.2"));
    let between = &s[header_indices[0]..header_indices[1]];
    assert!(
        between.ends_with("\n\n"),
        "blank line between stanzas: {between:?}"
    );
}

#[test]
fn preview_text_emits_skipped_lines_when_flag_is_set() {
    let output = PreviewOutput {
        series: vec![SyncOutput {
            name: "rabbitmq-main".into(),
            branch: Some("main".into()),
            pins: Vec::new(),
            dropped_unconfigured: Vec::new(),
            skipped: vec![SkippedPin {
                name: "3bad".into(),
                reason: "name not a valid project identifier".into(),
            }],
        }],
        show_skipped: true,
    };
    let s = render(&output);
    assert!(s.contains("# skipped 3bad:"), "{s}");
}

#[test]
fn preview_text_hides_skipped_lines_when_flag_is_not_set() {
    let output = PreviewOutput {
        series: vec![SyncOutput {
            name: "rabbitmq-main".into(),
            branch: Some("main".into()),
            pins: Vec::new(),
            dropped_unconfigured: Vec::new(),
            skipped: vec![SkippedPin {
                name: "3bad".into(),
                reason: "name not a valid project identifier".into(),
            }],
        }],
        show_skipped: false,
    };
    let s = render(&output);
    assert!(!s.contains("# skipped"), "{s}");
}

#[test]
fn default_branches_match_supported_release_branches() {
    let branches = default_branches();
    assert!(branches.contains(&"main".to_string()));
    assert!(branches.iter().any(|b| b.starts_with("v4.")));
    assert_eq!(branches.len(), DEFAULT_BRANCHES.len());
}

#[test]
fn series_name_for_branch_strips_refs_tags_prefix() {
    assert_eq!(series_name_for_branch("refs/tags/v4.2.0"), "rabbitmq-4.2.0");
}
