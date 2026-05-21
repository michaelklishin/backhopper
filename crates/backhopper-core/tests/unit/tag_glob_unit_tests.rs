// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::model::names::{TagGlob, TagName};

fn tag(s: &str) -> TagName {
    TagName::new(s).expect("valid tag")
}

fn glob(s: &str) -> TagGlob {
    TagGlob::new(s).expect("valid tag glob")
}

#[test]
fn star_matches_anything() {
    assert!(glob("*").matches(&tag("OTP-26.2.5")));
    assert!(glob("*").matches(&tag("v1")));
}

#[test]
fn prefix_then_star_matches_versions_with_that_prefix() {
    let g = glob("OTP-26.*");
    assert!(g.matches(&tag("OTP-26.0")));
    assert!(g.matches(&tag("OTP-26.2.5")));
    assert!(g.matches(&tag("OTP-26.99.99")));
    assert!(!g.matches(&tag("OTP-25.3")));
    assert!(!g.matches(&tag("OTP-27.0")));
}

#[test]
fn literal_pattern_only_matches_exact_tag() {
    let g = glob("OTP-26.2.5");
    assert!(g.matches(&tag("OTP-26.2.5")));
    assert!(!g.matches(&tag("OTP-26.2.6")));
}

#[test]
fn question_mark_matches_single_character() {
    let g = glob("v?.0");
    assert!(g.matches(&tag("v1.0")));
    assert!(g.matches(&tag("v9.0")));
    assert!(!g.matches(&tag("v10.0")));
}

#[test]
fn trailing_star_consumes_the_rest() {
    let g = glob("OTP-*");
    assert!(g.matches(&tag("OTP-26")));
    assert!(g.matches(&tag("OTP-26.2.5-rc1")));
}

#[test]
fn empty_glob_rejected_at_construction() {
    assert!(TagGlob::new("").is_err());
}

#[test]
fn glob_forbids_path_separators() {
    assert!(TagGlob::new("OTP/26").is_err());
    assert!(TagGlob::new("OTP\\26").is_err());
}

#[test]
fn glob_forbids_git_ref_magic() {
    assert!(TagGlob::new("..").is_err());
    assert!(TagGlob::new(".OTP").is_err());
    assert!(TagGlob::new("OTP-26.").is_err());
}

#[test]
fn glob_allows_wildcards_in_charset() {
    assert!(TagGlob::new("*").is_ok());
    assert!(TagGlob::new("?").is_ok());
    assert!(TagGlob::new("OTP-26.*").is_ok());
}

#[test]
fn from_str_round_trips() {
    let g = TagGlob::from_str("OTP-27.*").unwrap();
    assert_eq!(g.as_str(), "OTP-27.*");
    assert_eq!(g.to_string(), "OTP-27.*");
}
