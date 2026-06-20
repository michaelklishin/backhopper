// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::resolver_coverage::{ResolverClass, coverage};

#[test]
fn every_class_is_covered() {
    assert!(
        ResolverClass::ALL
            .into_iter()
            .all(ResolverClass::is_covered)
    );
}

#[test]
fn coverage_lists_every_class_once() {
    let listed = coverage();
    assert_eq!(listed.len(), ResolverClass::ALL.len());
    assert!(listed.iter().all(|(_, covered)| *covered));
}

#[test]
fn class_round_trips_through_serde() {
    let s = serde_json::to_string(&ResolverClass::Macro).unwrap();
    assert_eq!(s, "\"macro\"");
    let back: ResolverClass = serde_json::from_str(&s).unwrap();
    assert_eq!(back, ResolverClass::Macro);
}
