// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The corpus fold: joins recorded verdicts to observed build outcomes
//! and reports how trustworthy the verdict is. The fingerprint is what
//! pairs the two; this is a plain fold over the paired rows, run as
//! needed, not a stored subsystem.

use serde::{Deserialize, Serialize};

use crate::model::fingerprint::VerdictFingerprint;
use crate::model::resolver_coverage::ResolverClass;

/// The verdict a row carried, collapsed to whether it flagged the pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VerdictClass {
    Compatible,
    Inapplicable,
    RequiresAdaptation,
    Incompatible,
}

impl VerdictClass {
    /// True when the verdict told the operator to adapt or stop: the
    /// other arms (`Compatible`, `Inapplicable`) read as "nothing here".
    #[must_use]
    pub fn flagged(self) -> bool {
        matches!(self, Self::RequiresAdaptation | Self::Incompatible)
    }
}

/// What the build did once the pick landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuildOutcome {
    BuiltClean,
    CompileFailed,
    ApplyConflicted,
    TestRegressed,
}

impl BuildOutcome {
    #[must_use]
    pub fn is_break(self) -> bool {
        !matches!(self, Self::BuiltClean)
    }
}

/// One paired row: a verdict and the outcome it was meant to predict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CorpusEntry {
    pub fingerprint: VerdictFingerprint,
    pub verdict: VerdictClass,
    pub outcome: BuildOutcome,
    /// The symbol class a break fell in, when known: lets a missed break
    /// be routed to a coverage gap or a resolver bug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_class: Option<ResolverClass>,
}

/// A measured rate kept as `hit/total` so the sample size is never lost
/// behind a bare percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Ratio {
    pub hits: usize,
    pub total: usize,
}

/// A break the verdict did not flag: a resolver bug if its class is
/// covered, a coverage gap if not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MissedBreak {
    pub fingerprint: VerdictFingerprint,
    pub break_class: Option<ResolverClass>,
    /// `true` when the class is one the resolver claims to cover, so the
    /// miss is a bug rather than an unbuilt class.
    pub is_resolver_bug: bool,
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
    let mut by_class: std::collections::BTreeMap<ResolverClass, usize> =
        std::collections::BTreeMap::new();
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
            if let Some(class) = e.break_class {
                *by_class.entry(class).or_insert(0) += 1;
            }
            if !flagged {
                missed.push(MissedBreak {
                    fingerprint: e.fingerprint.clone(),
                    break_class: e.break_class,
                    is_resolver_bug: e.break_class.is_some_and(ResolverClass::is_covered),
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
