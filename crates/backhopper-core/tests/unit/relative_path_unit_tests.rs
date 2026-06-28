// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::errors::NameError;
use backhopper_core::model::names::RelativePath;

#[test]
fn accepts_ordinary_repo_relative_paths() {
    RelativePath::new("deps/rabbit/src/rabbit.erl").expect("valid");
    RelativePath::new("Cargo.toml").expect("valid");
    RelativePath::new("deps/osiris/src/osiris_log.erl").expect("valid");
}

#[test]
fn rejects_empty() {
    assert!(matches!(
        RelativePath::new(""),
        Err(NameError::Empty { .. })
    ));
}

#[test]
fn rejects_absolute_paths() {
    let err = RelativePath::new("/etc/passwd").expect_err("absolute rejected");
    assert!(matches!(err, NameError::Invalid { .. }));
}

#[test]
fn rejects_parent_directory_segments() {
    let err = RelativePath::new("a/../../etc").expect_err("escape rejected");
    assert!(matches!(err, NameError::Invalid { .. }));
}

#[test]
fn rejects_isolated_dot_dot_segment() {
    assert!(RelativePath::new("..").is_err());
    assert!(RelativePath::new("../rabbit.erl").is_err());
}

#[test]
fn rejects_nul_bytes() {
    let err = RelativePath::new("a\0b").expect_err("NUL rejected");
    assert!(matches!(err, NameError::Invalid { .. }));
}

#[test]
fn rejects_overlong_paths() {
    let long = "x/".repeat(2200);
    assert!(matches!(
        RelativePath::new(long),
        Err(NameError::TooLong { .. })
    ));
}

#[test]
fn permits_single_dot_segments() {
    RelativePath::new("./Cargo.toml").expect("valid");
    RelativePath::new("src/./rabbit.erl").expect("valid");
}

#[test]
fn from_str_roundtrip() {
    let p = RelativePath::from_str("deps/rabbit/src/rabbit.erl").expect("valid");
    assert_eq!(p.as_str(), "deps/rabbit/src/rabbit.erl");
}

#[test]
fn display_matches_inner_string() {
    let p = RelativePath::new("deps/aten/src/aten.erl").expect("valid");
    assert_eq!(p.to_string(), "deps/aten/src/aten.erl");
}

#[test]
fn from_bytes_accepts_utf8_bytes() {
    let p = RelativePath::from_bytes(b"deps/rabbit/src/rabbit.erl").expect("utf-8 bytes parse");
    assert_eq!(p.as_str(), "deps/rabbit/src/rabbit.erl");
}

#[test]
fn from_bytes_rejects_invalid_utf8() {
    let err = RelativePath::from_bytes(&[0x66, 0x6F, 0x80, 0x6F]).expect_err("non-utf-8 rejected");
    assert!(matches!(err, NameError::Invalid { .. }));
}

#[test]
fn from_bytes_rejects_absolute_path() {
    let err = RelativePath::from_bytes(b"/etc/passwd").expect_err("absolute rejected");
    assert!(matches!(err, NameError::Invalid { .. }));
}

#[test]
fn serde_roundtrip_via_json() {
    let p = RelativePath::new("deps/seshat/src/seshat.erl").expect("valid");
    let json = serde_json::to_string(&p).expect("serialise");
    assert_eq!(json, "\"deps/seshat/src/seshat.erl\"");
    let back: RelativePath = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, p);
}
