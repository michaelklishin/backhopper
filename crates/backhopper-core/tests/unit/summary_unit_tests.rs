// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::model::names::CommitSha;
use backhopper_core::model::summary::{SummaryRow, VerdictKind};
use backhopper_core::model::verdict::{Reason, TouchedKinds, Verdict};

#[test]
fn verdict_kind_from_verdict_maps_every_variant() {
    assert_eq!(
        VerdictKind::from(&Verdict::Compatible),
        VerdictKind::Compatible
    );
    assert_eq!(
        VerdictKind::from(&Verdict::Incompatible { reasons: vec![] }),
        VerdictKind::Incompatible
    );
    assert_eq!(
        VerdictKind::from(&Verdict::RequiresAdaptation { reasons: vec![] }),
        VerdictKind::RequiresAdaptation
    );
}

#[test]
fn verdict_kind_serialises_as_snake_case() {
    for (k, s) in [
        (VerdictKind::Compatible, "\"compatible\""),
        (VerdictKind::RequiresAdaptation, "\"requires_adaptation\""),
        (VerdictKind::Incompatible, "\"incompatible\""),
        (VerdictKind::Inapplicable, "\"inapplicable\""),
    ] {
        assert_eq!(serde_json::to_string(&k).unwrap(), s);
    }
}

#[test]
fn verdict_kind_as_str_matches_serde_rename() {
    assert_eq!(VerdictKind::Compatible.as_str(), "compatible");
    assert_eq!(
        VerdictKind::RequiresAdaptation.as_str(),
        "requires_adaptation"
    );
    assert_eq!(VerdictKind::Incompatible.as_str(), "incompatible");
    assert_eq!(VerdictKind::Inapplicable.as_str(), "inapplicable");
}

#[test]
fn summary_row_round_trips_via_json() {
    let row = SummaryRow {
        sha: CommitSha::new("a".repeat(40)).unwrap(),
        verdict: VerdictKind::Compatible,
        touched: TouchedKinds {
            erl: 2,
            tests: 1,
            ..TouchedKinds::default()
        },
        tracked: 7,
        subject: "Add osiris bloom filter".to_owned(),
        series: None,
        parent_count: None,
        apply_conflicts: 0,
    };
    let json = serde_json::to_string(&row).unwrap();
    let back: SummaryRow = serde_json::from_str(&json).unwrap();
    assert_eq!(back, row);
}

#[test]
fn summary_row_json_includes_touched_breakdown() {
    let row = SummaryRow {
        sha: CommitSha::new("b".repeat(40)).unwrap(),
        verdict: VerdictKind::Inapplicable,
        touched: TouchedKinds {
            erl: 1,
            ..TouchedKinds::default()
        },
        tracked: 0,
        subject: "Update CHANGELOG".to_owned(),
        series: None,
        parent_count: None,
        apply_conflicts: 0,
    };
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains("\"erl\":1"));
    assert!(json.contains("\"tracked\":0"));
    assert!(json.contains("\"verdict\":\"inapplicable\""));
}

#[test]
fn unused_reason_variant_still_present() {
    let _: Verdict = Verdict::Incompatible {
        reasons: vec![Reason::FileAbsent {
            path: PathBuf::from("deps/ra/src/ra_log.erl"),
        }],
    };
}
