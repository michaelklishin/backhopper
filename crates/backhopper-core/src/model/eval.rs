// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The corpus fold: joins recorded verdicts to observed build outcomes
//! and reports how trustworthy the verdict is. The fingerprint pairs the
//! two; this is a plain fold over the paired rows, run as needed, not a
//! stored subsystem.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::evaluation::AggregateVerdict;
use crate::model::fingerprint::VerdictFingerprint;
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
/// behind a bare percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Ratio {
    pub hits: usize,
    pub total: usize,
}

/// A break the verdict did not flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MissedBreak {
    pub fingerprint: VerdictFingerprint,
    pub break_class: Option<ResolverClass>,
    pub outcome: BuildOutcome,
    /// `Some(true)` a resolver bug (the producer checked the class),
    /// `Some(false)` a coverage gap, `None` when unknown: the row recorded
    /// no coverage, or the break carried no class.
    pub is_resolver_bug: Option<bool>,
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
    /// Breaks a "nothing here" verdict missed, the harvest worklist.
    pub missed_breaks: Vec<MissedBreak>,
}

/// Fold the paired corpus into accuracy rates and a missed-break list.
#[must_use]
pub fn evaluate_corpus(entries: &[CorpusEntry]) -> EvalReport {
    let mut vacuous_trust = Ratio { hits: 0, total: 0 };
    let mut recall = Ratio { hits: 0, total: 0 };
    let mut precision = Ratio { hits: 0, total: 0 };
    let mut by_class: BTreeMap<ResolverClass, usize> = BTreeMap::new();
    let mut missed = Vec::new();

    for e in entries {
        let broke = e.outcome.is_break();
        let flagged = e.verdict.flagged();
        if !flagged {
            vacuous_trust.total += 1;
            if !broke {
                vacuous_trust.hits += 1;
            }
        }
        if broke {
            recall.total += 1;
            if flagged {
                recall.hits += 1;
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
                    is_resolver_bug: resolver_bug(break_class, e.coverage.as_ref()),
                });
            }
        }
        if flagged {
            precision.total += 1;
            if broke {
                precision.hits += 1;
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
fn resolver_bug(class: Option<ResolverClass>, coverage: Option<&ResolverCoverage>) -> Option<bool> {
    match (class, coverage) {
        (Some(class), Some(coverage)) => Some(coverage.is_checked(class)),
        _ => None,
    }
}
