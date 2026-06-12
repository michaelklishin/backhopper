// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A recorded pre-v7 `check batch` envelope must keep parsing after
//! the v7 additions: `parent_count` was not on the wire then, so it
//! deserialises as `None` and `pr_commits` semantics stay intact.

use backhopper_core::model::batch::BatchPayload;
use backhopper_driver::SUPPORTED_SCHEMA;
use backhopper_driver::envelope::SchemaVersion;

/// The v6 wire shape: one plain-commit row, no `parent_count` key
/// anywhere.
const V6_BATCH_ENVELOPE_DATA: &str = r#"{
  "queried_against": [
    {
      "series": "stable",
      "pins": [{ "project": "demo", "tag": "v1.0.0" }]
    }
  ],
  "results": [
    {
      "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "series": "stable",
      "verdict": {
        "results": [],
        "summary": {
          "compatible": 0,
          "requires_adaptation": 0,
          "incompatible": 0,
          "inapplicable": 0
        }
      },
      "touched_paths": [],
      "pr_commits": null
    }
  ]
}"#;

#[test]
fn v6_batch_payload_parses_with_parent_count_none() {
    let payload: BatchPayload =
        serde_json::from_str(V6_BATCH_ENVELOPE_DATA).expect("v6 payload parses under v7 types");
    let row = &payload.results[0];
    assert_eq!(row.parent_count, None);
    assert_eq!(row.pr_commits, None);
}

#[test]
fn v6_is_inside_the_supported_schema_range() {
    assert!(SUPPORTED_SCHEMA.contains(SchemaVersion::new(6)));
    assert!(SUPPORTED_SCHEMA.contains(SchemaVersion::new(9)));
    assert!(SUPPORTED_SCHEMA.contains(SchemaVersion::new(10)));
}

#[test]
fn pr_commits_none_vs_empty_distinction_survives_round_trip() {
    let payload: BatchPayload = serde_json::from_str(V6_BATCH_ENVELOPE_DATA).unwrap();
    let json = serde_json::to_value(&payload).unwrap();
    assert!(json["results"][0]["pr_commits"].is_null());
    // v7 serialisers always emit the key; absent and null read the same
    assert!(json["results"][0]["parent_count"].is_null());
}
