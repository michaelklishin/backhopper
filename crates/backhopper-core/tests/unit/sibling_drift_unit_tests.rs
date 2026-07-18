// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

// exact float equality is the contract under test: the clamp emits exactly 0.0 and 1.0
#![allow(clippy::float_cmp)]

use std::num::NonZeroU32;

use time::OffsetDateTime;

use backhopper_core::model::names::RelativePath;
use backhopper_core::model::sibling_drift::{
    ActionThresholds, CandidateFeatures, Confidence, ScoreWeights, SiblingCandidate,
    SiblingDriftAction, SinceDerivation, Unscored, Vocabulary, is_test_path, score_features,
};

fn vocab(terms: &[&str]) -> Vocabulary {
    let owned: Vec<String> = terms.iter().map(|s| (*s).to_owned()).collect();
    Vocabulary::compile(&owned).unwrap()
}

fn rel(path: &str) -> RelativePath {
    path.parse().unwrap()
}

fn features(subject: &str, paths: &[&str], added: u32, removed: u32) -> CandidateFeatures {
    CandidateFeatures {
        subject: subject.to_owned(),
        touched_paths: paths.iter().map(|p| rel(p)).collect(),
        lines_added: added,
        lines_removed: removed,
        parent_count: NonZeroU32::MIN,
    }
}

// --- Confidence ---

#[test]
fn confidence_clamps_into_the_unit_interval() {
    assert_eq!(Confidence::new(-0.5).value(), 0.0);
    assert_eq!(Confidence::new(1.5).value(), 1.0);
    assert_eq!(Confidence::new(0.42).value(), 0.42);
}

#[test]
fn confidence_maps_nan_to_zero() {
    assert_eq!(Confidence::new(f32::NAN).value(), 0.0);
}

#[test]
fn confidence_orders_totally() {
    let mut values = [
        Confidence::new(0.9),
        Confidence::new(0.1),
        Confidence::new(0.5),
    ];
    values.sort();
    let as_f32: Vec<f32> = values.iter().map(|c| c.value()).collect();
    assert_eq!(as_f32, vec![0.1, 0.5, 0.9]);
}

#[test]
fn confidence_displays_with_two_decimals() {
    assert_eq!(Confidence::new(0.876).to_string(), "0.88");
    assert_eq!(Confidence::new(0.0).to_string(), "0.00");
}

#[test]
fn confidence_deserialisation_clamps_wire_input() {
    let c: Confidence = serde_json::from_str("7.5").unwrap();
    assert_eq!(c.value(), 1.0);
}

// --- Vocabulary ---

#[test]
fn word_boundary_terms_match_prefixes_not_infixes() {
    let v = vocab(&["crash"]);
    assert_eq!(v.distinct_matches("Fix crashes in tests"), 1);
    assert_eq!(v.distinct_matches("crashing queues everywhere"), 1);
    assert_eq!(v.distinct_matches("petcrash is not a word boundary"), 0);
}

#[test]
fn non_word_leading_terms_match_as_substrings() {
    let v = vocab(&["_SUITE"]);
    assert_eq!(v.distinct_matches("Fix crashing_queues_SUITE"), 1);
}

#[test]
fn matching_is_case_insensitive() {
    let v = vocab(&["flake"]);
    assert_eq!(v.distinct_matches("FLAKE fix"), 1);
    assert_eq!(v.distinct_matches("Deal with a Flake"), 1);
}

#[test]
fn density_counts_distinct_terms_not_raw_hits() {
    let v = vocab(&["flake", "crash"]);
    // one term repeated five times is still one distinct match
    assert_eq!(v.distinct_matches("flake flake flake flake flake"), 1);
    assert_eq!(v.distinct_matches("a flaky crash flake"), 2);
}

#[test]
fn compile_skips_blank_and_duplicate_terms() {
    let v = vocab(&["flake", "", "  ", "FLAKE", "crash"]);
    assert_eq!(v.len(), 2);
    assert_eq!(v.terms(), &["flake".to_owned(), "crash".to_owned()]);
}

#[test]
fn empty_vocabulary_reports_empty_and_never_matches() {
    let v = vocab(&[]);
    assert!(v.is_empty());
    assert_eq!(v.distinct_matches("flake crash _SUITE"), 0);
}

#[test]
fn multi_word_phrase_terms_match() {
    let v = vocab(&["mixed version"]);
    assert_eq!(
        v.distinct_matches("Skip tests in mixed version clusters"),
        1
    );
    assert_eq!(v.distinct_matches("mixed-version clusters"), 0);
}

// --- is_test_path ---

#[test]
fn suite_files_and_test_directories_are_test_paths() {
    assert!(is_test_path(&rel(
        "deps/rabbit/test/quorum_queue_SUITE.erl"
    )));
    assert!(is_test_path(&rel("deps/rabbit/src/some_SUITE.erl")));
    assert!(is_test_path(&rel(
        "deps/rabbitmq_ct_helpers/src/rabbit_ct_helpers.erl"
    )));
    assert!(is_test_path(&rel("tests/thing.erl")));
}

#[test]
fn production_paths_are_not_test_paths() {
    assert!(!is_test_path(&rel("deps/rabbit/src/rabbit_fifo.erl")));
    assert!(!is_test_path(&rel("src/contest.erl")));
}

// --- scorer factors ---

#[test]
fn zero_vocabulary_matches_means_zero_confidence() {
    let v = vocab(&["flake"]);
    let f = features("Add a new feature", &["deps/rabbit/test/x_SUITE.erl"], 2, 1);
    let outcome = score_features(&f, &v, &ScoreWeights::default());
    assert_eq!(outcome.confidence.value(), 0.0);
    assert_eq!(outcome.components.vocabulary_terms_matched, 0);
}

#[test]
fn line_count_factor_decays_with_size_and_never_reaches_zero() {
    let v = vocab(&["flake"]);
    let small = features("flake", &["test/a_SUITE.erl"], 2, 2);
    let large = features("flake", &["test/a_SUITE.erl"], 4000, 2000);
    let w = ScoreWeights::default();
    let small_outcome = score_features(&small, &v, &w);
    let large_outcome = score_features(&large, &v, &w);
    assert!(small_outcome.confidence > large_outcome.confidence);
    assert!(large_outcome.confidence.value() > 0.0);
    assert!(small_outcome.components.line_count_factor > 0.9);
}

#[test]
fn test_path_share_raises_the_test_path_factor() {
    let v = vocab(&["flake"]);
    let w = ScoreWeights::default();
    let all_test = features("flake", &["test/a_SUITE.erl"], 2, 2);
    let mixed = features("flake", &["test/a_SUITE.erl", "src/app.erl"], 2, 2);
    let none = features("flake", &["src/app.erl"], 2, 2);
    let f_all = score_features(&all_test, &v, &w)
        .components
        .test_path_factor;
    let f_mixed = score_features(&mixed, &v, &w).components.test_path_factor;
    let f_none = score_features(&none, &v, &w).components.test_path_factor;
    assert_eq!(f_all, 1.0);
    assert_eq!(f_none, w.test_path_floor);
    assert!(f_none < f_mixed && f_mixed < f_all);
}

#[test]
fn empty_path_list_scores_at_the_test_path_floor() {
    let v = vocab(&["flake"]);
    let w = ScoreWeights::default();
    let f = features("flake", &[], 0, 0);
    assert_eq!(
        score_features(&f, &v, &w).components.test_path_factor,
        w.test_path_floor
    );
}

#[test]
fn more_distinct_terms_never_score_lower() {
    let v = vocab(&["flake", "crash", "race"]);
    let w = ScoreWeights::default();
    let one = score_features(&features("flake", &["test/a_SUITE.erl"], 2, 2), &v, &w);
    let two = score_features(
        &features("flake crash", &["test/a_SUITE.erl"], 2, 2),
        &v,
        &w,
    );
    let three = score_features(
        &features("flake crash race", &["test/a_SUITE.erl"], 2, 2),
        &v,
        &w,
    );
    assert!(one.confidence < two.confidence);
    assert!(two.confidence < three.confidence);
}

// --- action thresholds ---

#[test]
fn threshold_bands_are_half_open() {
    let t = ActionThresholds::default();
    assert_eq!(
        t.action_for(Confidence::new(0.75)),
        SiblingDriftAction::Sweep
    );
    assert_eq!(
        t.action_for(Confidence::new(0.7499)),
        SiblingDriftAction::InvestigateInPlace
    );
    assert_eq!(
        t.action_for(Confidence::new(0.40)),
        SiblingDriftAction::InvestigateInPlace
    );
    assert_eq!(
        t.action_for(Confidence::new(0.3999)),
        SiblingDriftAction::Ignore
    );
    assert_eq!(
        t.action_for(Confidence::new(0.0)),
        SiblingDriftAction::Ignore
    );
    assert_eq!(
        t.action_for(Confidence::new(1.0)),
        SiblingDriftAction::Sweep
    );
}

// --- type-state pipeline ---

fn unscored_candidate() -> SiblingCandidate<Unscored> {
    SiblingCandidate {
        sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
        subject: "Fix a flake in crashing_queues_SUITE".to_owned(),
        committed_at: OffsetDateTime::from_unix_timestamp(1_750_000_000).unwrap(),
        age_days: 0,
        touched_paths: vec![rel("deps/rabbit/test/crashing_queues_SUITE.erl")],
        parent_count: NonZeroU32::MIN,
        state: Unscored {
            lines_added: 3,
            lines_removed: 1,
        },
    }
}

#[test]
fn scoring_fills_confidence_action_and_components() {
    let scored = unscored_candidate().score(
        &vocab(&["flake", "crash", "_SUITE"]),
        &ScoreWeights::default(),
        &ActionThresholds::default(),
    );
    assert!(scored.confidence().value() > 0.9);
    assert_eq!(scored.action(), SiblingDriftAction::Sweep);
    assert!(scored.state.score_components.is_some());
}

#[test]
fn without_components_strips_only_the_breakdown() {
    let scored = unscored_candidate().score(
        &vocab(&["flake"]),
        &ScoreWeights::default(),
        &ActionThresholds::default(),
    );
    let confidence = scored.confidence();
    let stripped = scored.without_components();
    assert!(stripped.state.score_components.is_none());
    assert_eq!(stripped.confidence(), confidence);
}

#[test]
fn scored_candidate_serialises_with_flattened_state() {
    let scored = unscored_candidate().score(
        &vocab(&["flake"]),
        &ScoreWeights::default(),
        &ActionThresholds::default(),
    );
    let json = serde_json::to_value(&scored).unwrap();
    // state fields sit at the row level, not under a nested key
    assert!(json.get("confidence").is_some());
    assert!(json.get("action").is_some());
    assert!(json.get("state").is_none());
}

// --- SinceDerivation ---

#[test]
fn since_derivation_exposes_its_sha_and_tags_on_the_wire() {
    let sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".parse().unwrap();
    let since = SinceDerivation::ExplicitSha { sha };
    assert_eq!(
        since.sha().as_str(),
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    let json = serde_json::to_value(&since).unwrap();
    assert_eq!(json["kind"], "explicit_sha");
}
