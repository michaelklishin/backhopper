// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Wire shape of the canonical `CheckPayload`: the empty-optional skip
//! rules and the always-emitted fields must match the `check` envelope
//! the CLI ships and the driver parses.

use backhopper_core::model::check_payload::{CheckPayload, QueriedAgainst};
use serde_json::json;

#[test]
fn check_payload_round_trips_and_keeps_the_skip_rules() {
    // a captured check envelope's data section
    let wire = json!({
        "queried_against": { "kind": "pin", "project": "ra", "tag": "v3.1.6" },
        "results": {
            "results": [],
            "summary": { "compatible": 0, "requires_adaptation": 0, "incompatible": 0 }
        },
        "pr_commits": null,
        "self_projects": [],
        "resolver_coverage": null,
        "fingerprint_version": 7
    });

    let payload: CheckPayload = serde_json::from_value(wire).expect("deserializes");
    assert!(matches!(
        payload.queried_against,
        QueriedAgainst::Pin { .. }
    ));
    assert_eq!(payload.fingerprint_version, Some(7));

    let re = serde_json::to_value(&payload).expect("serializes");
    let obj = re.as_object().expect("object");
    // empty or absent fields drop off the wire
    for skipped in [
        "diagnostics",
        "project_suggestions",
        "apply",
        "target_findings",
        "verdict_fingerprint",
    ] {
        assert!(!obj.contains_key(skipped), "{skipped} should be omitted");
    }
    // these are always emitted so a consumer can tell empty from an old binary
    for always in [
        "pr_commits",
        "self_projects",
        "resolver_coverage",
        "fingerprint_version",
    ] {
        assert!(obj.contains_key(always), "{always} should be emitted");
    }
    // the typed pin serializes back to the same bare strings
    assert_eq!(
        re["queried_against"],
        json!({ "kind": "pin", "project": "ra", "tag": "v3.1.6" })
    );
}
