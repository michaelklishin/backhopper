// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::ModuleName;
use backhopper_core::suites::SuiteInclusionReason;

#[test]
fn behaviour_implementer_sweep_variant_roundtrips_through_json() {
    let r = SuiteInclusionReason::BehaviourImplementerSweep {
        behaviour: ModuleName::new("rabbit_backing_queue").unwrap(),
        implementer: ModuleName::new("rabbit_variable_queue").unwrap(),
    };
    let j = serde_json::to_string(&r).expect("serialize");
    assert!(j.contains("\"kind\":\"behaviour_implementer_sweep\""));
    let parsed: SuiteInclusionReason = serde_json::from_str(&j).expect("deserialize");
    assert_eq!(r, parsed);
}

#[test]
fn variant_is_distinct_from_same_app_caller() {
    let sweep = SuiteInclusionReason::BehaviourImplementerSweep {
        behaviour: ModuleName::new("b").unwrap(),
        implementer: ModuleName::new("i").unwrap(),
    };
    if let SuiteInclusionReason::SameAppCaller { .. } = sweep {
        panic!("BehaviourImplementerSweep matched SameAppCaller pattern");
    }
}
