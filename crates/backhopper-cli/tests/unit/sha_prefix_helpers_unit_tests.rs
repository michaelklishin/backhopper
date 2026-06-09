// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use backhopper_cli::commands::sha_prefix::{expand_prefix, resolve_with_kind};
use backhopper_cli::errors::CliError;
use backhopper_core::model::names::{CommitSha, CommitShaPrefix};

#[test]
fn expand_prefix_short_circuits_full_form_without_opening_repo() {
    let full = "a".repeat(40);
    let prefix = CommitShaPrefix::new(&full).unwrap();
    let bogus_path = Path::new("/nonexistent/path/that/does/not/exist");
    let out: CommitSha = expand_prefix(bogus_path, &prefix).expect("full prefix short-circuits");
    assert_eq!(out.as_str(), &full);
}

#[test]
fn expand_prefix_on_short_form_against_missing_repo_surfaces_input_invalid_error() {
    let prefix = CommitShaPrefix::new("abc1234").unwrap();
    let bogus_path = Path::new("/nonexistent/path/that/does/not/exist");
    let err = expand_prefix(bogus_path, &prefix).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not a git repository") || matches!(err, CliError::Core(_)),
        "expected typed git error, got: {msg}"
    );
}

#[test]
fn resolve_with_kind_on_missing_repo_returns_not_a_git_repository() {
    let prefix = CommitShaPrefix::new("abc1234").unwrap();
    let bogus_path = Path::new("/nonexistent/path/that/does/not/exist");
    let err = resolve_with_kind(bogus_path, &prefix).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not a git repository"),
        "expected NotAGitRepository, got: {msg}"
    );
}
