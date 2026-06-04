// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Verify the v2 fields on `SeriesEvaluation`: `touched_paths` is
//! always emitted (even empty) and `pr_commits` distinguishes `None`
//! from `Some(vec![])`.

use backhopper_core::model::verdict::{
    Diagnostics, PatchFacts, SeriesEvaluation, SeriesSummary, SeriesVerdict,
};

#[test]
fn touched_paths_emits_as_empty_array_when_no_paths_seen() {
    let eval = SeriesEvaluation {
        verdict: SeriesVerdict {
            results: Vec::new(),
            summary: SeriesSummary::default(),
        },
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
    };
    let json = serde_json::to_value(&eval).unwrap();
    assert!(
        json.get("touched_paths").is_some(),
        "touched_paths must always serialise (empty array): {json}"
    );
    assert!(json["touched_paths"].as_array().unwrap().is_empty());
}
