// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `vocabulary!`'s round trip, walked once per enum: `label`, `Display`,
//! `FromStr` and the `serde_json` string all agree per variant, and
//! `FromStr` refuses a label with the case changed.

use std::fmt::{Debug, Display};
use std::str::FromStr;

use backhopper_core::config::{Language, ProjectFamily, ProjectKind, ProjectLayout};
use backhopper_core::model::eval::BreakAttribution;
use backhopper_core::model::evaluation::AggregateVerdict;
use backhopper_core::model::resolver_coverage::ResolverClass;
use backhopper_core::model::summary::VerdictKind;
use backhopper_core::model::verdict::ApplyConflictKind;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn round_trips<V>(all: &[V])
where
    V: Copy + Display + FromStr + Serialize + DeserializeOwned + PartialEq + Debug,
{
    for &variant in all {
        let json = serde_json::to_string(&variant).expect("serializes");
        let wire_label: String = serde_json::from_str(&json).expect("wire form is a string");
        assert_eq!(variant.to_string(), wire_label, "label matches serde form");

        let parsed =
            V::from_str(&wire_label).unwrap_or_else(|_| panic!("{wire_label} parses back"));
        assert_eq!(parsed, variant, "FromStr recovers the same variant");

        let back: V = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, variant, "serde round trips the same variant");

        let uppercased = wire_label.to_uppercase();
        if uppercased != wire_label {
            assert!(
                V::from_str(&uppercased).is_err(),
                "FromStr refuses a case-changed label"
            );
        }
    }
}

#[test]
fn project_kind_round_trips() {
    round_trips(ProjectKind::ALL);
}

#[test]
fn project_kind_spells_self_once() {
    assert_eq!(ProjectKind::SelfRepo.label(), "self");
    let json = serde_json::to_string(&ProjectKind::SelfRepo).unwrap();
    assert_eq!(json, "\"self\"");
}

#[test]
fn project_family_round_trips() {
    round_trips(ProjectFamily::ALL);
}

#[test]
fn language_round_trips() {
    round_trips(Language::ALL);
}

#[test]
fn project_layout_round_trips() {
    round_trips(ProjectLayout::ALL);
}

#[test]
fn resolver_class_round_trips() {
    round_trips(ResolverClass::ALL);
}

#[test]
fn aggregate_verdict_round_trips() {
    round_trips(AggregateVerdict::ALL);
}

#[test]
fn verdict_kind_round_trips() {
    round_trips(VerdictKind::ALL);
}

#[test]
fn apply_conflict_kind_round_trips() {
    round_trips(ApplyConflictKind::ALL);
}

#[test]
fn break_attribution_round_trips() {
    round_trips(BreakAttribution::ALL);
}
