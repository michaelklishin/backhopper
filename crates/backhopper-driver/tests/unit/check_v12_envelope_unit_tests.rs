// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A recorded v12 `check` envelope carries `apply` without
//! `target_findings`: the field entered at v10, `target_findings` at
//! v14. It must keep deserialising into `CheckPayload` under today's
//! types, which is why the payload's `apply` and `target_findings`
//! stay flat `Option` fields rather than a slot.

use backhopper_core::model::check_payload::CheckPayload;
use backhopper_core::model::producer::Producer;

const V12_CHECK_ENVELOPE_DATA: &str = r#"{
  "queried_against": { "kind": "pin", "project": "ra", "tag": "v2.16.13" },
  "results": {
    "results": [],
    "summary": { "compatible": 0, "requires_adaptation": 0, "incompatible": 0 }
  },
  "self_projects": ["rabbit"],
  "resolver_coverage": { "checked": ["macro", "record"] },
  "fingerprint_version": 3,
  "apply": { "paths": {} }
}"#;

#[test]
fn v12_check_payload_parses_apply_without_target_findings() {
    let payload: CheckPayload =
        serde_json::from_str(V12_CHECK_ENVELOPE_DATA).expect("v12 payload parses under v14 types");
    assert!(payload.apply.is_some());
    assert_eq!(payload.target_findings, None);
}

#[test]
fn v12_check_payload_still_reads_as_a_current_producer() {
    let payload: CheckPayload = serde_json::from_str(V12_CHECK_ENVELOPE_DATA).unwrap();
    assert!(matches!(payload.producer(), Producer::Current { .. }));
}
