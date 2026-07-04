// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The `siblings doctor` scorer regression gate.
//!
//! The corpus stores `CandidateFeatures` extracted from real
//! `rabbitmq-server` first-parent commits (12 months of `main`),
//! hand-labeled `should_cascade`, `should_not`, or `unclear`. The
//! gate is hermetic: features, never SHAs, so it runs without a
//! RabbitMQ clone. Weight, threshold, or default-vocabulary drift
//! that pushes F1 or precision below the floors fails the build.

use std::num::NonZeroU32;

use serde::Deserialize;

use backhopper_core::config::ProjectFamily;
use backhopper_core::model::sibling_drift::{
    ActionThresholds, CandidateFeatures, ScoreWeights, SiblingDriftAction, Vocabulary,
    score_features,
};

/// Floors measured at calibration time (precision 0.98, recall 0.91,
/// F1 0.94), set with headroom for future relabeling. The design's
/// hard refuse-to-ship line is precision 0.50.
const F1_FLOOR: f64 = 0.85;
const PRECISION_FLOOR: f64 = 0.80;

#[derive(Debug, Deserialize)]
struct CorpusRow {
    label: Label,
    #[allow(dead_code)]
    note: String,
    features: CandidateFeatures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Label {
    ShouldCascade,
    ShouldNot,
    Unclear,
}

fn corpus() -> Vec<CorpusRow> {
    let raw = include_str!("../fixtures/sibling_drift_corpus/corpus.jsonl");
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("corpus line parses"))
        .collect()
}

fn rabbitmq_vocabulary() -> Vocabulary {
    let terms = ProjectFamily::Rabbitmq.defaults().sibling_drift_vocabulary;
    Vocabulary::compile(&terms).expect("default vocabulary compiles")
}

#[test]
fn corpus_has_enough_labeled_rows_to_be_meaningful() {
    let rows = corpus();
    let labeled = rows.iter().filter(|r| r.label != Label::Unclear).count();
    assert!(
        labeled >= 80,
        "labeled corpus shrank to {labeled} rows; the F1 gate needs at least 80"
    );
}

#[test]
fn scorer_clears_the_f1_and_precision_floors_on_the_labeled_corpus() {
    let vocabulary = rabbitmq_vocabulary();
    let weights = ScoreWeights::default();
    let thresholds = ActionThresholds::default();
    let (mut tp, mut fp, mut fn_) = (0u32, 0u32, 0u32);
    for row in corpus() {
        if row.label == Label::Unclear {
            continue;
        }
        let outcome = score_features(&row.features, &vocabulary, &weights);
        let surfaced = thresholds.action_for(outcome.confidence) != SiblingDriftAction::Ignore;
        let positive = row.label == Label::ShouldCascade;
        match (surfaced, positive) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fn_ += 1,
            (false, false) => {}
        }
    }
    let precision = f64::from(tp) / f64::from(tp + fp);
    let recall = f64::from(tp) / f64::from(tp + fn_);
    let f1 = 2.0 * precision * recall / (precision + recall);
    assert!(
        precision >= PRECISION_FLOOR,
        "precision {precision:.3} fell below the {PRECISION_FLOOR} floor \
         (tp={tp} fp={fp} fn={fn_})"
    );
    assert!(
        f1 >= F1_FLOOR,
        "F1 {f1:.3} fell below the {F1_FLOOR} floor \
         (precision={precision:.3} recall={recall:.3} tp={tp} fp={fp} fn={fn_})"
    );
}

/// The motivating shape: a small flag-only
/// `find_crashes` gate fix in a SUITE must land in the `Sweep` band.
#[test]
fn case_c_shaped_fix_lands_in_the_sweep_band() {
    let vocabulary = rabbitmq_vocabulary();
    let features = CandidateFeatures {
        subject: "Don't find queue crashes in logs in crashing_queues_SUITE".into(),
        touched_paths: vec![
            "deps/rabbit/test/crashing_queues_SUITE.erl"
                .parse()
                .unwrap(),
        ],
        lines_added: 4,
        lines_removed: 2,
        parent_count: NonZeroU32::MIN,
    };
    let outcome = score_features(&features, &vocabulary, &ScoreWeights::default());
    let action = ActionThresholds::default().action_for(outcome.confidence);
    assert_eq!(action, SiblingDriftAction::Sweep);
}
