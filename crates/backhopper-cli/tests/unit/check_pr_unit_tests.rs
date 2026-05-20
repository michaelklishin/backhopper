// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::commands::check::{PrCoordinates, parse_pr_url};

#[test]
fn parses_a_canonical_pr_url() {
    let c = parse_pr_url("https://github.com/rabbitmq/rabbitmq-server/pull/16367").unwrap();
    assert_eq!(
        c,
        PrCoordinates {
            owner: "rabbitmq".into(),
            repo: "rabbitmq-server".into(),
            number: 16367,
        }
    );
}

#[test]
fn http_scheme_is_also_accepted() {
    let c = parse_pr_url("http://github.com/foo/bar/pull/1").unwrap();
    assert_eq!(c.number, 1);
}

#[test]
fn trailing_segments_after_number_are_ignored() {
    let c = parse_pr_url("https://github.com/foo/bar/pull/42/files").unwrap();
    assert_eq!(c.number, 42);
    assert_eq!(c.repo, "bar");
}

#[test]
fn non_github_url_is_rejected() {
    assert!(parse_pr_url("https://gitlab.com/foo/bar/-/merge_requests/1").is_err());
}

#[test]
fn missing_pull_segment_is_rejected() {
    assert!(parse_pr_url("https://github.com/foo/bar/issues/42").is_err());
}

#[test]
fn missing_pr_number_is_rejected() {
    assert!(parse_pr_url("https://github.com/foo/bar/pull/").is_err());
}

#[test]
fn non_integer_pr_number_is_rejected() {
    assert!(parse_pr_url("https://github.com/foo/bar/pull/abc").is_err());
}

#[test]
fn empty_owner_is_rejected() {
    assert!(parse_pr_url("https://github.com//bar/pull/1").is_err());
}
