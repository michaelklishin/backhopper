// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Serde round-trips for the v7 wire additions: `SummaryRow.series`,
//! `SummaryRow.parent_count`, and `BatchResult.parent_count`,
//! including the `None` forms a pre-v7 producer leaves behind.

use std::num::NonZeroU32;

use proptest::prelude::*;

use backhopper_core::model::names::{CommitSha, SeriesName};
use backhopper_core::model::summary::{SummaryRow, VerdictKind};
use backhopper_core::model::verdict::TouchedKinds;

fn arb_sha() -> impl Strategy<Value = CommitSha> {
    "[0-9a-f]{40}".prop_map(|s| CommitSha::new(s).unwrap())
}

fn arb_series() -> impl Strategy<Value = Option<SeriesName>> {
    proptest::option::of("[a-z][a-z0-9-]{0,20}".prop_map(|s| SeriesName::new(s).unwrap()))
}

fn arb_parent_count() -> impl Strategy<Value = Option<NonZeroU32>> {
    proptest::option::of((1u32..=64).prop_map(|n| NonZeroU32::new(n).unwrap()))
}

fn arb_verdict_kind() -> impl Strategy<Value = VerdictKind> {
    prop_oneof![
        Just(VerdictKind::Compatible),
        Just(VerdictKind::RequiresAdaptation),
        Just(VerdictKind::Incompatible),
        Just(VerdictKind::Inapplicable),
    ]
}

proptest! {
    #[test]
    fn summary_row_round_trips_including_none_forms(
        sha in arb_sha(),
        verdict in arb_verdict_kind(),
        tracked in any::<u32>(),
        subject in ".{0,40}",
        series in arb_series(),
        parent_count in arb_parent_count(),
    ) {
        let row = SummaryRow {
            sha,
            verdict,
            touched: TouchedKinds::default(),
            tracked,
            subject,
            series,
            parent_count,
            apply_conflicts: 0,
            target_findings: 0,
        };
        let json = serde_json::to_string(&row).unwrap();
        let back: SummaryRow = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(row, back);
    }

    #[test]
    fn summary_row_missing_v7_keys_reads_as_none(
        sha in arb_sha(),
        tracked in any::<u32>(),
    ) {
        // hand-built pre-v7 shape: no series, no parent_count
        let legacy = format!(
            r#"{{"sha":"{sha}","verdict":"compatible","touched":{{}},"tracked":{tracked},"subject":"s"}}"#
        );
        let row: SummaryRow = serde_json::from_str(&legacy).unwrap();
        prop_assert_eq!(row.series, None);
        prop_assert_eq!(row.parent_count, None);
    }
}
