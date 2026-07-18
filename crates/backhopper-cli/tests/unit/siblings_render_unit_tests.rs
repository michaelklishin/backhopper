// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Text rendering of `siblings doctor`. The key pair is the
//! two zero-candidate runs: an empty vocabulary analysed nothing, a
//! populated one analysed everything and found nothing, and the text
//! must not read the same for both.

use std::num::NonZeroU32;

use bel7_cli::TableStyle;
use time::macros::datetime;

use backhopper_cli::commands::siblings::render_doctor_text;
use backhopper_core::model::names::{CommitSha, GitRef, RelativePath, SeriesName, TagName};
use backhopper_core::model::sibling_drift::{
    ActionThresholds, ScoreWeights, Scored, SiblingCandidate, SiblingDoctorReport, SinceDerivation,
    Unscored, Vocabulary, VocabularySource,
};

fn commit(c: char) -> CommitSha {
    CommitSha::new(c.to_string().repeat(40)).unwrap()
}

fn scored_candidate(subject: &str) -> SiblingCandidate<Scored> {
    let vocabulary = Vocabulary::compile(&["flake".to_owned(), "_SUITE".to_owned()]).unwrap();
    SiblingCandidate {
        sha: commit('a'),
        subject: subject.to_owned(),
        committed_at: datetime!(2026-06-15 10:00:00 UTC),
        age_days: 30,
        touched_paths: vec![RelativePath::new("deps/rabbit/test/backing_queue_SUITE.erl").unwrap()],
        parent_count: NonZeroU32::new(1).unwrap(),
        state: Unscored {
            lines_added: 12,
            lines_removed: 3,
        },
    }
    .score(
        &vocabulary,
        &ScoreWeights::default(),
        &ActionThresholds::default(),
    )
}

fn report(
    vocabulary_source: VocabularySource,
    candidates: Vec<SiblingCandidate<Scored>>,
) -> SiblingDoctorReport {
    SiblingDoctorReport {
        series: SeriesName::new("rabbitmq-4.0").unwrap(),
        target_branch: GitRef::new("v4.0.x").unwrap(),
        source_branches: vec![GitRef::new("origin/main").unwrap()],
        since: SinceDerivation::LastReleaseTag {
            tag: TagName::new("v4.0.9").unwrap(),
            sha: commit('b'),
        },
        vocabulary_source,
        suppressed_count: 0,
        walked_count: 7392,
        candidates,
    }
}

fn render(report: &SiblingDoctorReport) -> String {
    let mut out: Vec<u8> = Vec::new();
    render_doctor_text(&mut out, report, None, TableStyle::default()).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn an_empty_vocabulary_run_names_itself_as_bounding_nothing() {
    let text = render(&report(VocabularySource::Empty, Vec::new()));
    assert!(
        text.contains("no vocabulary in effect"),
        "expected the vacuity to be named, got: {text}"
    );
    assert!(text.contains("does not bound sibling-drift risk"), "{text}");
}

#[test]
fn a_populated_vocabulary_with_no_candidates_renders_the_plain_line() {
    for source in [VocabularySource::FamilyDefault, VocabularySource::File] {
        let text = render(&report(source, Vec::new()));
        assert!(text.contains("no sibling-drift candidates found"), "{text}");
        assert!(
            !text.contains("no vocabulary in effect"),
            "a populated vocabulary that found nothing must not read as vacuous, got: {text}"
        );
    }
}

#[test]
fn the_two_zero_candidate_runs_do_not_render_the_same_text() {
    let vacuous = render(&report(VocabularySource::Empty, Vec::new()));
    let clean = render(&report(VocabularySource::FamilyDefault, Vec::new()));
    assert_ne!(vacuous, clean);
}

#[test]
fn a_candidate_table_is_unaffected_by_the_vocabulary_source() {
    let candidates = vec![scored_candidate("Fix a flaky backing_queue_SUITE case")];
    let text = render(&report(VocabularySource::FamilyDefault, candidates));
    assert!(text.contains("backing_queue_SUITE"), "{text}");
    assert!(
        !text.contains("no sibling-drift candidates found"),
        "{text}"
    );
    assert!(!text.contains("no vocabulary in effect"), "{text}");
}
