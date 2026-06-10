// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Types and the pure scorer behind `siblings doctor`.
//!
//! A candidate moves through a one-way type-state pipeline:
//! `SiblingCandidate<Unscored>` carries raw diff facts, `score`
//! converts it into `SiblingCandidate<Scored>`, and only the scored
//! form serialises. The scorer itself is a pure function over
//! `CandidateFeatures`, so the labeled corpus gate and the proptests
//! run without a git repository.

use std::cmp::Ordering;
use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize};
use time::OffsetDateTime;

use crate::model::names::{CommitSha, GitRef, RelativePath, SeriesName, TagName};

/// A clamped `[0.0, 1.0]` ranking score. Total-ordered via
/// `f32::total_cmp` so candidates always sort; the constructor maps
/// NaN to zero, so no surprising order exists to begin with.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(transparent)
)]
pub struct Confidence(f32);

impl Confidence {
    #[must_use]
    pub fn new(value: f32) -> Self {
        if value.is_nan() {
            return Self(0.0);
        }
        Self(value.clamp(0.0, 1.0))
    }

    #[must_use]
    pub fn value(self) -> f32 {
        self.0
    }
}

impl PartialEq for Confidence {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == Ordering::Equal
    }
}

impl Eq for Confidence {}

impl PartialOrd for Confidence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Confidence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

// route wire input through the clamping constructor
impl<'de> Deserialize<'de> for Confidence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::new(f32::deserialize(deserializer)?))
    }
}

/// Recommended operator action, computed from threshold bands here,
/// never re-derived by consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SiblingDriftAction {
    Sweep,
    InvestigateInPlace,
    Ignore,
}

impl SiblingDriftAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sweep => "sweep",
            Self::InvestigateInPlace => "investigate_in_place",
            Self::Ignore => "ignore",
        }
    }
}

/// How the since-point was derived. Travels on the wire so consumers
/// can tell a defaulted window from an operator-chosen one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SinceDerivation {
    LastReleaseTag { tag: TagName, sha: CommitSha },
    ExplicitTag { tag: TagName, sha: CommitSha },
    ExplicitSha { sha: CommitSha },
}

impl SinceDerivation {
    #[must_use]
    pub fn sha(&self) -> &CommitSha {
        match self {
            Self::LastReleaseTag { sha, .. }
            | Self::ExplicitTag { sha, .. }
            | Self::ExplicitSha { sha } => sha,
        }
    }
}

/// Where the effective vocabulary came from. `Empty` is emitted (with
/// a warning) instead of silently producing a zero-candidate run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VocabularySource {
    FamilyDefault,
    File,
    Empty,
}

/// Compiled subject-prefilter vocabulary. Matching is case-insensitive
/// and word-boundary anchored when a term starts with a word
/// character, so `crash` hits `crashes` but not `scratch`; terms with
/// a non-word leading character (`_SUITE`) match as plain substrings.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    terms: Vec<String>,
    set: regex::RegexSet,
}

impl Vocabulary {
    /// Compile `terms`, skipping blanks and case-insensitive
    /// duplicates while preserving first-seen order.
    pub fn compile(terms: &[String]) -> Result<Self, regex::Error> {
        let mut kept: Vec<String> = Vec::with_capacity(terms.len());
        for term in terms {
            let trimmed = term.trim();
            if trimmed.is_empty() {
                continue;
            }
            if kept.iter().any(|k| k.eq_ignore_ascii_case(trimmed)) {
                continue;
            }
            kept.push(trimmed.to_owned());
        }
        let patterns: Vec<String> = kept.iter().map(|t| term_pattern(t)).collect();
        let set = regex::RegexSet::new(&patterns)?;
        Ok(Self { terms: kept, set })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    #[must_use]
    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    /// Count of distinct vocabulary terms the subject matches. Raw
    /// hit counts are deliberately not used: a keyword-stuffed
    /// subject must not outrank a real fix.
    #[must_use]
    pub fn distinct_matches(&self, subject: &str) -> u32 {
        u32::try_from(self.set.matches(subject).iter().count()).unwrap_or(u32::MAX)
    }
}

fn term_pattern(term: &str) -> String {
    let escaped = regex::escape(term);
    let starts_with_word = term
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    if starts_with_word {
        format!("(?i)\\b{escaped}")
    } else {
        format!("(?i){escaped}")
    }
}

/// The scorer's full input: everything extracted from one walked
/// commit. Serialisable so the labeled corpus stores features, not
/// SHAs, and the F1 gate runs hermetically in CI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CandidateFeatures {
    pub subject: String,
    pub touched_paths: Vec<RelativePath>,
    pub lines_added: u32,
    pub lines_removed: u32,
    pub parent_count: NonZeroU32,
}

/// Scoring knobs. Calibrated together with `ActionThresholds` against
/// the labeled corpus; changes ship as releases, and the crate version
/// is a cache freshness key, so a stale cache cannot serve old scores.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreWeights {
    /// Total changed lines at which the size factor halves.
    pub line_half_life: f32,
    /// Vocabulary factor for exactly one distinct term matched.
    pub vocabulary_single: f32,
    /// Vocabulary factor for exactly two distinct terms matched.
    pub vocabulary_double: f32,
    /// Test-path factor floor: applied when no touched path looks
    /// test-related; the factor climbs linearly to 1.0 as the
    /// test-path share grows.
    pub test_path_floor: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            line_half_life: 200.0,
            vocabulary_single: 0.7,
            vocabulary_double: 0.9,
            test_path_floor: 0.5,
        }
    }
}

/// Confidence bands for the recommended action. Half-open:
/// `confidence >= sweep_min` is `Sweep`, then
/// `confidence >= investigate_min` is `InvestigateInPlace`, the rest
/// is `Ignore`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionThresholds {
    pub sweep_min: Confidence,
    pub investigate_min: Confidence,
}

impl Default for ActionThresholds {
    fn default() -> Self {
        Self {
            sweep_min: Confidence::new(0.75),
            investigate_min: Confidence::new(0.40),
        }
    }
}

impl ActionThresholds {
    #[must_use]
    pub fn action_for(&self, confidence: Confidence) -> SiblingDriftAction {
        if confidence >= self.sweep_min {
            SiblingDriftAction::Sweep
        } else if confidence >= self.investigate_min {
            SiblingDriftAction::InvestigateInPlace
        } else {
            SiblingDriftAction::Ignore
        }
    }
}

/// Per-factor breakdown serialised under `--explain` for operator
/// trust and corpus tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ScoreComponents {
    pub line_count_factor: f32,
    pub vocabulary_terms_matched: u32,
    pub test_path_factor: f32,
}

/// What the pure scorer returns: the clamped confidence plus the
/// factor breakdown it was computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreOutcome {
    pub confidence: Confidence,
    pub components: ScoreComponents,
}

/// Score one candidate's features. Pure: same inputs, same score.
/// Age is deliberately not a factor; the oldest unswept fix is the
/// most overdue, not the least relevant.
#[must_use]
pub fn score_features(
    features: &CandidateFeatures,
    vocabulary: &Vocabulary,
    weights: &ScoreWeights,
) -> ScoreOutcome {
    let total_lines = features.lines_added.saturating_add(features.lines_removed);
    let line_count_factor = line_count_factor(total_lines, weights);
    let matched = vocabulary.distinct_matches(&features.subject);
    let vocabulary_factor = vocabulary_factor(matched, weights);
    let test_path_factor = test_path_factor(&features.touched_paths, weights);
    let confidence = Confidence::new(line_count_factor * vocabulary_factor * test_path_factor);
    ScoreOutcome {
        confidence,
        components: ScoreComponents {
            line_count_factor,
            vocabulary_terms_matched: matched,
            test_path_factor,
        },
    }
}

// smooth decay: 1.0 at zero lines, 0.5 at the half-life
fn line_count_factor(total_lines: u32, weights: &ScoreWeights) -> f32 {
    let half_life = weights.line_half_life.max(1.0);
    1.0 / (1.0 + (total_lines as f32) / half_life)
}

fn vocabulary_factor(distinct_matches: u32, weights: &ScoreWeights) -> f32 {
    match distinct_matches {
        0 => 0.0,
        1 => weights.vocabulary_single,
        2 => weights.vocabulary_double,
        _ => 1.0,
    }
}

fn test_path_factor(paths: &[RelativePath], weights: &ScoreWeights) -> f32 {
    let floor = weights.test_path_floor.clamp(0.0, 1.0);
    if paths.is_empty() {
        return floor;
    }
    let test_count = paths.iter().filter(|p| is_test_path(p)).count();
    let share = (test_count as f32) / (paths.len() as f32);
    floor + (1.0 - floor) * share
}

/// True for paths that look test-related: a `test`/`tests` directory
/// component, a Common Test suite file, or a CT helper application.
#[must_use]
pub fn is_test_path(path: &RelativePath) -> bool {
    let s = path.as_str();
    let file_is_suite = s
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains("_SUITE"));
    if file_is_suite {
        return true;
    }
    s.split('/')
        .any(|c| c == "test" || c == "tests" || c.contains("ct_helpers"))
}

mod sealed {
    pub trait Sealed {}
}

/// Marker trait for the candidate pipeline states. Sealed: the only
/// states are `Unscored` and `Scored`.
pub trait ScoreState: sealed::Sealed {}

/// Pre-scoring state: raw diff size facts, no confidence yet. Not
/// serialisable, so an unscored candidate can never reach a wire
/// formatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unscored {
    pub lines_added: u32,
    pub lines_removed: u32,
}

/// Post-scoring state: the wire fields every consumer reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Scored {
    pub confidence: Confidence,
    pub action: SiblingDriftAction,
    /// Present in memory after every scoring pass; the CLI strips it
    /// from output unless `--explain` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_components: Option<ScoreComponents>,
}

impl sealed::Sealed for Unscored {}
impl ScoreState for Unscored {}
impl sealed::Sealed for Scored {}
impl ScoreState for Scored {}

/// One sibling-branch commit moving through the doctor pipeline.
/// `state` is flattened on the wire, so a scored candidate serialises
/// with `confidence` and `action` as plain row fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SiblingCandidate<S: ScoreState> {
    pub sha: CommitSha,
    pub subject: String,
    /// Committer timestamp: when the commit landed on the source
    /// branch, which is what "how overdue is this sweep" measures.
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    #[serde(with = "time::serde::rfc3339")]
    pub committed_at: OffsetDateTime,
    /// Whole days between `committed_at` and the run; recomputed on
    /// cache hits so a cached row never reports a stale age.
    pub age_days: u32,
    pub touched_paths: Vec<RelativePath>,
    pub parent_count: NonZeroU32,
    #[serde(flatten)]
    pub state: S,
}

impl SiblingCandidate<Unscored> {
    /// Consume the unscored candidate and produce the scored form.
    /// The only way to obtain `SiblingCandidate<Scored>` outside
    /// deserialisation.
    #[must_use]
    pub fn score(
        self,
        vocabulary: &Vocabulary,
        weights: &ScoreWeights,
        thresholds: &ActionThresholds,
    ) -> SiblingCandidate<Scored> {
        let features = CandidateFeatures {
            subject: self.subject.clone(),
            touched_paths: self.touched_paths.clone(),
            lines_added: self.state.lines_added,
            lines_removed: self.state.lines_removed,
            parent_count: self.parent_count,
        };
        let outcome = score_features(&features, vocabulary, weights);
        SiblingCandidate {
            sha: self.sha,
            subject: self.subject,
            committed_at: self.committed_at,
            age_days: self.age_days,
            touched_paths: self.touched_paths,
            parent_count: self.parent_count,
            state: Scored {
                confidence: outcome.confidence,
                action: thresholds.action_for(outcome.confidence),
                score_components: Some(outcome.components),
            },
        }
    }
}

impl SiblingCandidate<Scored> {
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        self.state.confidence
    }

    #[must_use]
    pub fn action(&self) -> SiblingDriftAction {
        self.state.action
    }

    /// Drop the per-factor breakdown (the non-`--explain` shape).
    #[must_use]
    pub fn without_components(mut self) -> Self {
        self.state.score_components = None;
        self
    }
}

/// The `siblings doctor` envelope payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SiblingDoctorReport {
    pub series: SeriesName,
    pub target_branch: GitRef,
    pub source_branches: Vec<GitRef>,
    pub since: SinceDerivation,
    pub vocabulary_source: VocabularySource,
    /// Candidates dropped because they already cascaded to the
    /// target (`-x` trailer or patch-id match): the visible proof
    /// the suppression filter ran.
    pub suppressed_count: u32,
    /// First-parent commits walked across every source branch.
    pub walked_count: u32,
    /// Ranked best-first; truncated to `--top`.
    pub candidates: Vec<SiblingCandidate<Scored>>,
}
