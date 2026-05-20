// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::TagName;

use backhopper_cli::commands::snapshots::filter_tags_since;

fn tag(s: &str) -> TagName {
    TagName::new(s).unwrap()
}

#[test]
fn no_since_returns_input_unchanged() {
    let tags = vec![tag("v3.0.0"), tag("v2.0.0"), tag("v1.0.0")];
    let out = filter_tags_since(tags.clone(), None);
    assert_eq!(out.len(), 3);
}

#[test]
fn since_keeps_only_tags_at_or_newer_than_threshold() {
    let tags = vec![tag("v3.0.0"), tag("v2.0.0"), tag("v1.0.0")];
    let out = filter_tags_since(tags, Some(&tag("v2.0.0")));
    assert_eq!(
        out.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        vec!["v3.0.0", "v2.0.0"]
    );
}

#[test]
fn since_includes_exact_match() {
    let tags = vec![tag("v2.0.0"), tag("v1.0.0")];
    let out = filter_tags_since(tags, Some(&tag("v2.0.0")));
    assert_eq!(
        out.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        vec!["v2.0.0"]
    );
}

#[test]
fn since_excludes_older_tags() {
    let tags = vec![tag("v1.5.0"), tag("v1.0.0"), tag("v0.9.0")];
    let out = filter_tags_since(tags, Some(&tag("v1.5.0")));
    assert_eq!(
        out.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        vec!["v1.5.0"]
    );
}

#[test]
fn since_handles_patch_level_versions() {
    let tags = vec![tag("v1.2.10"), tag("v1.2.9"), tag("v1.2.0")];
    let out = filter_tags_since(tags, Some(&tag("v1.2.9")));
    assert_eq!(
        out.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        vec!["v1.2.10", "v1.2.9"]
    );
}

#[test]
fn since_newer_than_anything_in_list_yields_empty() {
    let tags = vec![tag("v1.0.0"), tag("v0.5.0")];
    let out = filter_tags_since(tags, Some(&tag("v9.9.9")));
    assert!(out.is_empty());
}

#[test]
fn since_older_than_anything_in_list_keeps_everything() {
    let tags = vec![tag("v3.0.0"), tag("v2.0.0"), tag("v1.0.0")];
    let out = filter_tags_since(tags, Some(&tag("v0.1.0")));
    assert_eq!(out.len(), 3);
}

#[test]
fn since_treats_unparseable_tag_consistently_with_others() {
    let tags = vec![tag("v1.0.0"), tag("nightly")];
    let _ = filter_tags_since(tags, Some(&tag("v0.5.0")));
}
