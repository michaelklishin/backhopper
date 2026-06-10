// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::num::NonZeroU32;

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::model::names::RelativePath;
use backhopper_core::model::sibling_drift::{
    ActionThresholds, CandidateFeatures, Confidence, ScoreWeights, Scored, SiblingCandidate,
    Unscored, Vocabulary, score_features,
};

fn arb_rel_path() -> impl Strategy<Value = RelativePath> {
    prop_oneof![
        Just("deps/rabbit/test/queue_SUITE.erl"),
        Just("deps/rabbit/src/rabbit_fifo.erl"),
        Just("deps/rabbitmq_ct_helpers/src/rabbit_ct_helpers.erl"),
        Just("src/app.erl"),
        Just("test/helper.erl"),
    ]
    .prop_map(|s: &str| s.parse().unwrap())
}

fn arb_features() -> impl Strategy<Value = CandidateFeatures> {
    (
        "[a-zA-Z0-9 _:&()-]{0,80}",
        prop::collection::vec(arb_rel_path(), 0..6),
        any::<u16>(),
        any::<u16>(),
        1u32..4,
    )
        .prop_map(
            |(subject, touched_paths, added, removed, parents)| CandidateFeatures {
                subject,
                touched_paths,
                lines_added: u32::from(added),
                lines_removed: u32::from(removed),
                parent_count: NonZeroU32::new(parents).unwrap(),
            },
        )
}

fn vocabulary() -> Vocabulary {
    let terms: Vec<String> = ["flake", "crash", "_SUITE", "race"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    Vocabulary::compile(&terms).unwrap()
}

proptest! {
    #[test]
    fn confidence_is_always_in_the_unit_interval(f in arb_features()) {
        let outcome = score_features(&f, &vocabulary(), &ScoreWeights::default());
        let v = outcome.confidence.value();
        prop_assert!((0.0..=1.0).contains(&v));
        prop_assert!(!v.is_nan());
    }

    #[test]
    fn confidence_constructor_clamps_any_input(raw in any::<f32>()) {
        let v = Confidence::new(raw).value();
        prop_assert!((0.0..=1.0).contains(&v));
        prop_assert!(!v.is_nan());
    }

    // an added vocabulary term never lowers confidence, all else fixed
    #[test]
    fn appending_a_vocabulary_term_is_monotone(f in arb_features()) {
        let vocab = vocabulary();
        let weights = ScoreWeights::default();
        let base = score_features(&f, &vocab, &weights).confidence;
        let mut enriched = f.clone();
        enriched.subject.push_str(" flake");
        let after = score_features(&enriched, &vocab, &weights).confidence;
        prop_assert!(after >= base);
    }

    // an added test-path match never lowers confidence, all else fixed
    #[test]
    fn appending_a_test_path_is_monotone(f in arb_features()) {
        let vocab = vocabulary();
        let weights = ScoreWeights::default();
        let base = score_features(&f, &vocab, &weights).confidence;
        let mut enriched = f.clone();
        enriched
            .touched_paths
            .push("deps/rabbit/test/extra_SUITE.erl".parse().unwrap());
        let after = score_features(&enriched, &vocab, &weights).confidence;
        prop_assert!(after >= base);
    }

    #[test]
    fn scored_candidates_round_trip_through_serde(f in arb_features(), ts in 1_000_000_000i64..2_000_000_000) {
        let unscored = SiblingCandidate {
            sha: "cccccccccccccccccccccccccccccccccccccccc".parse().unwrap(),
            subject: f.subject.clone(),
            committed_at: OffsetDateTime::from_unix_timestamp(ts).unwrap(),
            age_days: 12,
            touched_paths: f.touched_paths.clone(),
            parent_count: f.parent_count,
            state: Unscored {
                lines_added: f.lines_added,
                lines_removed: f.lines_removed,
            },
        };
        let scored = unscored.score(
            &vocabulary(),
            &ScoreWeights::default(),
            &ActionThresholds::default(),
        );
        let json = serde_json::to_string(&scored).unwrap();
        let back: SiblingCandidate<Scored> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, scored);
    }

    #[test]
    fn features_round_trip_through_serde(f in arb_features()) {
        let json = serde_json::to_string(&f).unwrap();
        let back: CandidateFeatures = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, f);
    }
}
