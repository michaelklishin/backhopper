// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::model::names::ModuleName;
use backhopper_core::suites::{SubstringMatcher, SuiteMatcher, text_references_module};

fn write(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn module(name: &str) -> ModuleName {
    ModuleName::new(name).unwrap()
}

#[test]
fn text_matcher_finds_top_level_remote_call() {
    assert!(text_references_module(
        "go() -> rabbit_db:set(1).",
        &module("rabbit_db")
    ));
}

#[test]
fn text_matcher_skips_substring_of_longer_name() {
    assert!(!text_references_module(
        "go() -> not_rabbit_db:set(1).",
        &module("rabbit_db")
    ));
}

#[test]
fn text_matcher_matches_at_start_of_text() {
    assert!(text_references_module(
        "rabbit_db:set(1).",
        &module("rabbit_db")
    ));
}

#[test]
fn text_matcher_does_not_match_when_module_absent() {
    assert!(!text_references_module(
        "go() -> other:fn(1).",
        &module("rabbit_db")
    ));
}

#[test]
fn text_matcher_returns_true_for_match_inside_string_literal() {
    assert!(text_references_module(
        "go() -> \"rabbit_db:set\".",
        &module("rabbit_db")
    ));
}

#[test]
fn substring_matcher_caches_file_content_across_calls() {
    let dir = TempDir::new().unwrap();
    let p = write(&dir, "vhost_SUITE.erl", "go() -> rabbit_db:set(1).");
    let mut matcher = SubstringMatcher::new();
    let first = matcher.modules_referenced_in_suite(&p, &[module("rabbit_db")]);
    std::fs::remove_file(&p).unwrap();
    let second = matcher.modules_referenced_in_suite(&p, &[module("rabbit_db")]);
    assert_eq!(first, second);
    assert!(first.contains(&module("rabbit_db")));
}

#[test]
fn substring_matcher_returns_empty_for_missing_file() {
    let mut matcher = SubstringMatcher::new();
    let refs = matcher.modules_referenced_in_suite(
        &PathBuf::from("/does/not/exist.erl"),
        &[module("rabbit_db")],
    );
    assert!(refs.is_empty());
}

#[test]
fn substring_matcher_returns_matching_subset() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "go() -> rabbit_db:set(1), khepri:put(2).",
    );
    let mut matcher = SubstringMatcher::new();
    let refs =
        matcher.modules_referenced_in_suite(&p, &[module("rabbit_db"), module("never_called")]);
    assert!(refs.contains(&module("rabbit_db")));
    assert!(!refs.contains(&module("never_called")));
}
