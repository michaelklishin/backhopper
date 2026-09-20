// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::compat::cuttlefish_features::{
    IntroducedMapping, check_schema_feature_unsupported, check_schema_key_reader_missing,
};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{CommitSha, GitRef, RelativePath};
use backhopper_core::model::verdict::Reason;

fn mapping(
    schema: &str,
    conf_key: Option<&str>,
    target: Option<&str>,
    attrs: &[&str],
) -> IntroducedMapping {
    IntroducedMapping {
        schema_path: RelativePath::new(schema).unwrap(),
        conf_key: conf_key.map(str::to_owned),
        mapping_target: target.map(str::to_owned),
        attr_names: attrs.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[test]
fn alias_fires_against_3_6_pin() {
    let m = mapping("rabbit.schema", Some("x"), Some("rabbit.x"), &["alias"]);
    let reasons = check_schema_feature_unsupported(&[m], Some("3.6.0"));
    assert!(matches!(
        reasons.as_slice(),
        [Reason::SchemaFeatureUnsupportedOnPin { .. }]
    ));
}

#[test]
fn alias_silent_against_3_7_pin_exactly() {
    let m = mapping("rabbit.schema", Some("x"), Some("rabbit.x"), &["alias"]);
    let reasons = check_schema_feature_unsupported(&[m], Some("3.7.0"));
    assert!(
        reasons.is_empty(),
        "floor is inclusive-supported, got {reasons:?}"
    );
}

#[test]
fn alias_silent_against_newer_pin() {
    let m = mapping("rabbit.schema", Some("x"), Some("rabbit.x"), &["alias"]);
    let reasons = check_schema_feature_unsupported(&[m], Some("3.8.0"));
    assert!(reasons.is_empty());
}

#[test]
fn unpinned_cuttlefish_is_silent() {
    let m = mapping("rabbit.schema", Some("x"), Some("rabbit.x"), &["alias"]);
    let reasons = check_schema_feature_unsupported(&[m], None);
    assert!(reasons.is_empty());
}

#[test]
fn non_floored_attribute_never_fires() {
    let m = mapping("rabbit.schema", Some("x"), Some("rabbit.x"), &["datatype"]);
    let reasons = check_schema_feature_unsupported(&[m], Some("1.0.0"));
    assert!(reasons.is_empty());
}

fn rabbit_app_target_index() -> TargetTreeIndex {
    let mut present: BTreeSet<PathBuf> = BTreeSet::new();
    present.insert(PathBuf::from("deps/rabbit/src/rabbit.erl"));
    TargetTreeIndex::from_parts(
        PathBuf::from("/tmp/target"),
        GitRef::new("HEAD").unwrap(),
        CommitSha::new("0".repeat(40)).unwrap(),
        present,
    )
}

fn empty_target_index() -> TargetTreeIndex {
    TargetTreeIndex::from_parts(
        PathBuf::from("/tmp/target"),
        GitRef::new("HEAD").unwrap(),
        CommitSha::new("0".repeat(40)).unwrap(),
        BTreeSet::new(),
    )
}

#[test]
fn reader_missing_fires_for_unread_key() {
    let target = rabbit_app_target_index();
    let m = mapping(
        "deps/rabbit/priv/schema/rabbit.schema",
        Some("x"),
        Some("rabbit.message_interceptors"),
        &[],
    );
    let read_target = |_: &RelativePath| None;
    let reasons = check_schema_key_reader_missing(&[m], &target, &read_target);
    assert!(matches!(
        reasons.as_slice(),
        [Reason::SchemaKeyReaderMissing { .. }]
    ));
}

#[test]
fn reader_present_is_silent() {
    let target = rabbit_app_target_index();
    let m = mapping(
        "deps/rabbit/priv/schema/rabbit.schema",
        Some("x"),
        Some("rabbit.message_interceptors"),
        &[],
    );
    let read_target =
        |_: &RelativePath| Some("get_env(rabbit, message_interceptors, [])".to_owned());
    let reasons = check_schema_key_reader_missing(&[m], &target, &read_target);
    assert!(reasons.is_empty(), "expected silence, got {reasons:?}");
}

#[test]
fn no_mapping_target_is_silent() {
    let target = empty_target_index();
    let m = mapping("rabbit.schema", Some("x"), None, &[]);
    let read_target = |_: &RelativePath| None;
    let reasons = check_schema_key_reader_missing(&[m], &target, &read_target);
    assert!(reasons.is_empty());
}

#[test]
fn absent_target_app_recorded() {
    let target = empty_target_index();
    let m = mapping(
        "deps/rabbit/priv/schema/rabbit.schema",
        Some("x"),
        Some("not_shipped_app.key"),
        &[],
    );
    let read_target = |_: &RelativePath| None;
    let reasons = check_schema_key_reader_missing(&[m], &target, &read_target);
    let Reason::SchemaKeyReaderMissing { absent_apps, .. } = &reasons[0] else {
        panic!("expected SchemaKeyReaderMissing, got {reasons:?}");
    };
    assert_eq!(absent_apps.len(), 1);
    assert_eq!(absent_apps[0].as_str(), "not_shipped_app");
}
