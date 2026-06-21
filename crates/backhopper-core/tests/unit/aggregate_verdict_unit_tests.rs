// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `AggregateVerdict::flagged` and its serde, both new to the corpus
//! fold's join.

use backhopper_core::model::evaluation::AggregateVerdict;

#[test]
fn only_incompatible_and_requires_adaptation_are_flagged() {
    assert!(AggregateVerdict::Incompatible.flagged());
    assert!(AggregateVerdict::RequiresAdaptation.flagged());
    assert!(!AggregateVerdict::Compatible.flagged());
    assert!(!AggregateVerdict::Inapplicable.flagged());
    // an empty series predicted nothing, so it reads as "nothing here"
    assert!(!AggregateVerdict::Empty.flagged());
}

#[test]
fn round_trips_through_serde_as_snake_case() {
    for (verdict, wire) in [
        (AggregateVerdict::Compatible, "\"compatible\""),
        (
            AggregateVerdict::RequiresAdaptation,
            "\"requires_adaptation\"",
        ),
        (AggregateVerdict::Incompatible, "\"incompatible\""),
        (AggregateVerdict::Inapplicable, "\"inapplicable\""),
        (AggregateVerdict::Empty, "\"empty\""),
    ] {
        assert_eq!(serde_json::to_string(&verdict).unwrap(), wire);
        let back: AggregateVerdict = serde_json::from_str(wire).unwrap();
        assert_eq!(back, verdict);
    }
}
