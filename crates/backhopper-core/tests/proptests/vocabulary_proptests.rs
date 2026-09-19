// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Any string outside a `vocabulary!` enum's label set is refused by
//! `FromStr`.

use std::str::FromStr;

use backhopper_core::config::{Language, ProjectFamily, ProjectKind, ProjectLayout};
use backhopper_core::model::eval::BreakAttribution;
use backhopper_core::model::evaluation::AggregateVerdict;
use backhopper_core::model::resolver_coverage::ResolverClass;
use backhopper_core::model::summary::VerdictKind;
use backhopper_core::model::verdict::ApplyConflictKind;
use proptest::prelude::*;

fn not_a_label(s: &str, labels: &[&str]) -> bool {
    !labels.contains(&s)
}

macro_rules! refuses_unknown_labels {
    ($test_name:ident, $ty:ty) => {
        proptest! {
            #[test]
            fn $test_name(s in "[a-zA-Z0-9_]{0,32}") {
                let labels: Vec<&str> = <$ty>::ALL.iter().map(|v| v.label()).collect();
                prop_assume!(not_a_label(&s, &labels));
                prop_assert!(<$ty>::from_str(&s).is_err());
            }
        }
    };
}

refuses_unknown_labels!(project_kind_refuses_unknown_labels, ProjectKind);
refuses_unknown_labels!(project_family_refuses_unknown_labels, ProjectFamily);
refuses_unknown_labels!(language_refuses_unknown_labels, Language);
refuses_unknown_labels!(project_layout_refuses_unknown_labels, ProjectLayout);
refuses_unknown_labels!(resolver_class_refuses_unknown_labels, ResolverClass);
refuses_unknown_labels!(aggregate_verdict_refuses_unknown_labels, AggregateVerdict);
refuses_unknown_labels!(verdict_kind_refuses_unknown_labels, VerdictKind);
refuses_unknown_labels!(
    apply_conflict_kind_refuses_unknown_labels,
    ApplyConflictKind
);
refuses_unknown_labels!(break_attribution_refuses_unknown_labels, BreakAttribution);
