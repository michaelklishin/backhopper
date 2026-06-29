// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Rejection and edge-acceptance paths for the name newtypes, using real
//! RabbitMQ-ecosystem values: tag globs over ra release lines, git refs,
//! quoted and protocol-suffixed Erlang names, wire-macro names, and
//! vendored dependency names and versions.

use backhopper_core::model::names::{
    DependencyName, DependencyVersion, FunctionName, GitRef, MacroName, TagGlob, TagName, TypeName,
};

#[test]
fn tag_glob_accepts_wildcards_but_rejects_paths_and_ref_magic() {
    assert!(TagGlob::new("v3.*").is_ok());
    assert!(TagGlob::new("v4.x").is_ok());
    assert!(TagGlob::new("v3/1").is_err());
    assert!(TagGlob::new("..").is_err());
    assert!(TagGlob::new(".hidden").is_err());
    assert!(TagGlob::new("trailing.").is_err());
    assert!(TagGlob::new("a:b").is_err());
    assert!(TagGlob::new("a^b").is_err());
    assert!(TagGlob::new("a~b").is_err());
    assert!(TagGlob::new("a[b").is_err());
    assert!(TagGlob::new("has space").is_err());
}

#[test]
fn git_ref_accepts_branches_and_shas_but_rejects_colon_and_space() {
    assert!(GitRef::new("main").is_ok());
    assert!(GitRef::new("v4.1.x").is_ok());
    assert!(GitRef::new("0".repeat(40)).is_ok());
    assert!(GitRef::new("refs:tags").is_err());
    assert!(GitRef::new("with space").is_err());
}

#[test]
fn erlang_name_accepts_quoted_and_protocol_suffix_forms() {
    // quoted atoms are accepted verbatim
    assert!(TypeName::new("'Some Quoted Type'").is_ok());
    // a trailing ? or ! is a protocol-style suffix
    assert!(FunctionName::new("ready?").is_ok());
    assert!(FunctionName::new("send!").is_ok());
    // @ is legal mid-atom
    assert!(FunctionName::new("node@host").is_ok());
    assert!(FunctionName::new("handle_call").is_ok());
}

#[test]
fn erlang_name_rejects_uppercase_lead_and_dashes() {
    assert!(TypeName::new("BadStart").is_err());
    assert!(FunctionName::new("bad-name").is_err());
    assert!(FunctionName::new("").is_err());
}

#[test]
fn macro_name_accepts_uppercase_wire_constants_rejects_digit_lead_and_dash() {
    assert!(MacroName::new("RA_PROTO_VERSION").is_ok());
    assert!(MacroName::new("MAGIC").is_ok());
    assert!(MacroName::new("1BAD").is_err());
    assert!(MacroName::new("BAD-NAME").is_err());
}

#[test]
fn dependency_name_is_lowercase_only() {
    assert!(DependencyName::new("rabbitmq_management").is_ok());
    assert!(DependencyName::new("aten").is_ok());
    assert!(DependencyName::new("RabbitMQ").is_err());
    assert!(DependencyName::new("has-dash").is_err());
}

#[test]
fn dependency_version_accepts_semver_and_rejects_whitespace() {
    assert!(DependencyVersion::new("1.2.3").is_ok());
    assert!(DependencyVersion::new("0.9.1-rc1").is_ok());
    assert!(DependencyVersion::new("3.13.0+build.7").is_ok());
    assert!(DependencyVersion::new("1.2 3").is_err());
}

#[test]
fn names_reject_values_over_the_length_cap() {
    let huge = "a".repeat(8192);
    assert!(TagName::new(huge.clone()).is_err());
    assert!(TypeName::new(huge.clone()).is_err());
    assert!(MacroName::new(huge).is_err());
}
