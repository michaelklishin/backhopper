// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The corpus fold: joins recorded verdicts to observed build outcomes
//! and reports how trustworthy the verdict is. The fingerprint pairs the
//! two; this is a plain fold over the paired rows, run as needed, not a
//! stored subsystem.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::evaluation::AggregateVerdict;
use crate::model::fingerprint::VerdictFingerprint;
use crate::model::names::{CommitSha, RelativePath, vocabulary};
use crate::model::resolver_coverage::{ResolverClass, ResolverCoverage};
use crate::model::verdict::ApplyConflictKind;

/// What the build did once the pick landed. Only `CompilationFailed` carries
/// a class: an apply conflict and a test regression are not symbol-class
/// breaks. The class is the symbol that actually broke, which the
/// consumer reads from the build error, not from the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuildOutcome {
    BuiltClean,
    CompilationFailed { class: Option<ResolverClass> },
    ApplyConflicted,
    TestRegressed,
}

impl BuildOutcome {
    #[must_use]
    pub fn is_break(self) -> bool {
        !matches!(self, Self::BuiltClean)
    }

    /// The symbol class the break fell in, for a compile failure that
    /// named one.
    #[must_use]
    pub fn break_class(self) -> Option<ResolverClass> {
        match self {
            Self::CompilationFailed { class } => class,
            Self::BuiltClean | Self::ApplyConflicted | Self::TestRegressed => None,
        }
    }
}

/// One paired row: a verdict, its fingerprint, and the outcome it was
/// meant to predict. `coverage` is the producing binary's claims, so the
/// fold routes bug-vs-gap against the row, not the running binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CorpusEntry {
    pub fingerprint: VerdictFingerprint,
    pub verdict: AggregateVerdict,
    pub outcome: BuildOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<ResolverCoverage>,
}

/// A risky apply conflict a verdict predicts: one path, its conflict
/// kind. The path is the reason's own `PathBuf`, never dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PredictedConflict {
    pub path: PathBuf,
    pub kind: ApplyConflictKind,
}

/// A measured rate kept as `hit/total` so the sample size is never lost
/// behind a bare percentage. `hits <= total` always: every mutator
/// grows `total` by at least as much as `hits` in the same step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Ratio {
    hits: usize,
    total: usize,
}

impl Ratio {
    #[must_use]
    pub const fn zero() -> Self {
        Self { hits: 0, total: 0 }
    }

    pub fn hit(&mut self) {
        self.hits += 1;
        self.total += 1;
    }

    pub fn miss(&mut self) {
        self.total += 1;
    }

    #[must_use]
    pub fn hits(&self) -> usize {
        self.hits
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.total
    }

    #[must_use]
    pub fn rate(&self) -> Option<f64> {
        if self.total == 0 {
            None
        } else {
            Some(self.hits as f64 / self.total as f64)
        }
    }

    /// Count `hits` matches out of `total` candidates in one step, for
    /// a fold over a whole slice. Private: `hit` and `miss` are the only
    /// public way to grow a ratio.
    fn extend(&mut self, hits: usize, total: usize) {
        debug_assert!(hits <= total, "a ratio's hits cannot exceed its total");
        self.hits += hits;
        self.total += total;
    }
}

vocabulary!(
    /// Who is responsible for a break the verdict did not flag: the
    /// resolver (it checked the class and still missed it), a coverage gap
    /// (it never checked the class), or unknown (the row recorded no
    /// coverage, or the break carried no class).
    pub enum BreakAttribution: "break attribution" {
        ResolverBug => "resolver_bug",
        CoverageGap => "coverage_gap",
        Unknown => "unknown",
    }
);

/// A break the verdict did not flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MissedBreak {
    pub fingerprint: VerdictFingerprint,
    pub break_class: Option<ResolverClass>,
    pub outcome: BuildOutcome,
    pub attribution: BreakAttribution,
}

/// The fold's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct EvalReport {
    pub rows: usize,
    /// Of rows whose verdict said "nothing here", the share that built
    /// clean: the number that authorises lightening verification.
    pub vacuous_trust: Ratio,
    /// Of real breaks, the share the verdict flagged.
    pub recall: Ratio,
    /// Of flagged rows, the share that were real breaks.
    pub precision: Ratio,
    /// Break count per symbol class, for ranking what to build next.
    pub breaks_by_class: Vec<(ResolverClass, usize)>,
    /// Breaks a "nothing here" verdict missed, the worklist.
    pub missed_breaks: Vec<MissedBreak>,
}

/// Fold the paired corpus into accuracy rates and a missed-break list.
#[must_use]
pub fn evaluate_corpus(entries: &[CorpusEntry]) -> EvalReport {
    let mut vacuous_trust = Ratio::zero();
    let mut recall = Ratio::zero();
    let mut precision = Ratio::zero();
    let mut by_class: BTreeMap<ResolverClass, usize> = BTreeMap::new();
    let mut missed = Vec::new();

    for e in entries {
        let broke = e.outcome.is_break();
        let flagged = e.verdict.flagged();
        if !flagged {
            if broke {
                vacuous_trust.miss();
            } else {
                vacuous_trust.hit();
            }
        }
        if broke {
            if flagged {
                recall.hit();
            } else {
                recall.miss();
            }
            let break_class = e.outcome.break_class();
            if let Some(class) = break_class {
                *by_class.entry(class).or_insert(0) += 1;
            }
            if !flagged {
                missed.push(MissedBreak {
                    fingerprint: e.fingerprint.clone(),
                    break_class,
                    outcome: e.outcome,
                    attribution: break_attribution(break_class, e.coverage.as_ref()),
                });
            }
        }
        if flagged {
            if broke {
                precision.hit();
            } else {
                precision.miss();
            }
        }
    }

    EvalReport {
        rows: entries.len(),
        vacuous_trust,
        recall,
        precision,
        breaks_by_class: by_class.into_iter().collect(),
        missed_breaks: missed,
    }
}

/// Bug-vs-gap against the row's own coverage: unknown when the class or
/// the coverage is absent.
fn break_attribution(
    class: Option<ResolverClass>,
    coverage: Option<&ResolverCoverage>,
) -> BreakAttribution {
    match (class, coverage) {
        (Some(class), Some(coverage)) if coverage.is_checked(class) => {
            BreakAttribution::ResolverBug
        }
        (Some(_), Some(_)) => BreakAttribution::CoverageGap,
        _ => BreakAttribution::Unknown,
    }
}

/// An apply conflict a forecast predicts: one path, its conflict kind.
/// Normalized to `RelativePath` by the producer, unlike
/// `PredictedConflict`, which keeps a lossless `PathBuf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ForecastedConflict {
    pub path: RelativePath,
    pub kind: ApplyConflictKind,
}

/// How the pick actually applied. `OutOfBand` landings correlate with
/// hard conflicts, so dropping them would bias recall toward easy picks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservedApply {
    Clean,
    /// Still a conflict when `paths` is empty: normalization may have
    /// dropped every path.
    Conflicted {
        paths: BTreeSet<RelativePath>,
    },
    OutOfBand,
}

/// One paired row: what triage predicted for a candidate and how the
/// apply went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ForecastEntry {
    pub sha: CommitSha,
    /// One conflict per path, the shape `predicted_conflicts` emits;
    /// duplicate paths would inflate the `path_overlap` denominator.
    pub predicted: Vec<ForecastedConflict>,
    pub observed: ObservedApply,
    /// Observed paths that did not normalize to `RelativePath`: counted
    /// so the report states what the producer dropped.
    #[serde(default)]
    pub unconvertible_paths: usize,
}

/// Observed conflicting paths the forecast did not predict, the
/// worklist row. Path-level, so a pick whose forecast called one path
/// and missed another still surfaces the missed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ForecastMiss {
    pub sha: CommitSha,
    /// Observed conflicting paths with no matched prediction.
    pub missed_paths: BTreeSet<RelativePath>,
    /// The entry's full prediction, for routing the miss. Empty means
    /// the forecast predicted nothing at all on this pick.
    pub predicted: Vec<ForecastedConflict>,
}

/// The forecast fold's output. The rates are entry-level: a hit is an
/// entry that predicted any conflict and observed any conflict; which
/// paths matched is graded by `path_overlap` and `false_negatives`.
/// Forecasts are computed at triage against the then-current target
/// tip, so false negatives partly measure tip drift, not forecast error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ForecastReport {
    pub entries: usize,
    /// Of conflict-predicting entries, the share that observed a conflict.
    pub precision: Ratio,
    /// Of conflict-observing entries, the share the forecast predicted.
    pub recall: Ratio,
    /// Of predicted paths on true-positive entries, the share matched by
    /// an observed conflicting path.
    pub path_overlap: Ratio,
    /// Entries excluded from both rate denominators: when this count
    /// dominates, the rates are not trustworthy.
    pub out_of_band: usize,
    /// Observed paths the producer could not normalize, summed.
    pub unconvertible_paths: usize,
    /// Observed conflicting paths with no matched prediction, per entry,
    /// including entries the rates count as hits.
    pub false_negatives: Vec<ForecastMiss>,
}

/// Fold paired forecast rows into accuracy rates and a missed-conflict
/// worklist.
#[must_use]
pub fn evaluate_forecasts(entries: &[ForecastEntry]) -> ForecastReport {
    let mut precision = Ratio::zero();
    let mut recall = Ratio::zero();
    let mut path_overlap = Ratio::zero();
    let mut out_of_band = 0;
    let mut unconvertible_paths = 0;
    let mut false_negatives = Vec::new();

    for e in entries {
        unconvertible_paths += e.unconvertible_paths;
        let observed_paths = match &e.observed {
            ObservedApply::OutOfBand => {
                out_of_band += 1;
                continue;
            }
            ObservedApply::Clean => None,
            ObservedApply::Conflicted { paths } => Some(paths),
        };
        let predicted = !e.predicted.is_empty();
        // the variant decides, not the path count: an all-unconvertible conflict still counts
        let observed = observed_paths.is_some();
        if predicted {
            if observed {
                precision.hit();
            } else {
                precision.miss();
            }
        }
        if observed {
            if predicted {
                recall.hit();
            } else {
                recall.miss();
            }
        }
        let Some(paths) = observed_paths else {
            continue;
        };
        let prediction_names = |o: &RelativePath| {
            e.predicted
                .iter()
                .any(|p| paths_match(p.path.as_str(), o.as_str()))
        };
        if predicted {
            let matched = e
                .predicted
                .iter()
                .filter(|p| {
                    paths
                        .iter()
                        .any(|o| paths_match(p.path.as_str(), o.as_str()))
                })
                .count();
            path_overlap.extend(matched, e.predicted.len());
        }
        let missed: BTreeSet<RelativePath> = paths
            .iter()
            .filter(|o| !prediction_names(o))
            .cloned()
            .collect();
        if !missed.is_empty() {
            false_negatives.push(ForecastMiss {
                sha: e.sha.clone(),
                missed_paths: missed,
                predicted: e.predicted.clone(),
            });
        }
    }

    ForecastReport {
        entries: entries.len(),
        precision,
        recall,
        path_overlap,
        out_of_band,
        unconvertible_paths,
        false_negatives,
    }
}

/// Whether two paths recorded relative to different roots name the same
/// file: equal, or one is a `/`-boundary suffix of the other. Many-to-one
/// by design: a bare `Makefile` matches any path ending in `/Makefile`.
#[must_use]
pub fn paths_match(a: &str, b: &str) -> bool {
    has_path_suffix(a, b) || has_path_suffix(b, a)
}

fn has_path_suffix(haystack: &str, needle: &str) -> bool {
    haystack == needle
        || (haystack.len() > needle.len()
            && haystack.ends_with(needle)
            && haystack.as_bytes()[haystack.len() - needle.len() - 1] == b'/')
}
