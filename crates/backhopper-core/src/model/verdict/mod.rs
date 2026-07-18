// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::apply::ApplyForecast;
use crate::model::findings::TargetFindings;
use crate::model::names::{
    Arity, CommitSha, DependencyName, FunctionName, ModuleName, ProjectName, RecordName,
    RelativePath, TagName,
};
use crate::model::pin::Pin;
use crate::model::pr_commit::PrCommit;
use crate::model::symbol::{SymbolKind, SymbolRef};

mod diagnostics;
mod reason;
mod touched;

pub use diagnostics::*;
pub use reason::*;
pub use touched::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PinVerdict {
    pub pin: Pin,
    pub verdict: Verdict,
    #[serde(default)]
    pub tracked_refs: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_ref_details: Vec<SymbolRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_delta_details: Vec<SourceDelta>,
    #[serde(default, skip_serializing_if = "TouchedKinds::is_empty")]
    pub touched: TouchedKinds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SourceDelta {
    pub module: ModuleName,
    pub function: FunctionName,
    pub arity: Arity,
    pub source_spec: String,
    pub target_spec: String,
}

impl PinVerdict {
    pub fn new(pin: Pin, verdict: Verdict) -> Self {
        Self {
            pin,
            verdict,
            tracked_refs: 0,
            tracked_ref_details: Vec::new(),
            source_delta_details: Vec::new(),
            touched: TouchedKinds::default(),
        }
    }

    #[must_use]
    pub fn with_tracked_refs(mut self, n: usize) -> Self {
        self.tracked_refs = n;
        self
    }

    #[must_use]
    pub fn with_tracked_ref_details(mut self, details: Vec<SymbolRef>) -> Self {
        self.tracked_refs = details.len();
        self.tracked_ref_details = details;
        self
    }

    #[must_use]
    pub fn with_source_delta_details(mut self, deltas: Vec<SourceDelta>) -> Self {
        self.source_delta_details = deltas;
        self
    }

    #[must_use]
    pub fn with_touched(mut self, touched: TouchedKinds) -> Self {
        self.touched = touched;
        self
    }
}

/// Saturating sum of `tracked_refs` over pins not owned by a self project.
#[must_use]
pub fn non_self_tracked(results: &[PinVerdict], self_projects: &BTreeSet<ProjectName>) -> u32 {
    results
        .iter()
        .filter(|pin| !self_projects.contains(&pin.pin.project))
        .fold(0u32, |acc, pin| {
            acc.saturating_add(u32::try_from(pin.tracked_refs).unwrap_or(u32::MAX))
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SeriesVerdict {
    pub results: Vec<PinVerdict>,
    pub summary: SeriesSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SeriesSummary {
    pub compatible: u32,
    pub requires_adaptation: u32,
    pub incompatible: u32,
    #[serde(default)]
    pub inapplicable: u32,
}

impl SeriesSummary {
    /// True when any pin needs attention: the one rule both the series
    /// exit code and the round clearance gate on.
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        self.incompatible > 0 || self.requires_adaptation > 0
    }
}

// 0 every pin clean; 3 partial-success when any pin needs attention
pub mod exit {
    pub const OK: i32 = 0;
    pub const NEEDS_ATTENTION: i32 = 3;
}

impl SeriesVerdict {
    pub fn from_results(results: Vec<PinVerdict>) -> Self {
        let summary = summary_of(&results);
        Self { results, summary }
    }

    /// Re-derives the summary after per-pin verdicts were mutated in
    /// place (path routing, availability reclassification).
    pub fn recompute_summary(&mut self) {
        self.summary = summary_of(&self.results);
    }

    /// Promote each `Compatible`-with-zero-refs pin to `Inapplicable` when its
    /// `TouchedKinds` indicates no analyzable Erlang surface. The summary
    /// is recomputed from the rewritten results.
    pub fn promote_inapplicable(self) -> Self {
        let results: Vec<PinVerdict> = self
            .results
            .into_iter()
            .map(|pv| {
                if matches!(pv.verdict, Verdict::Compatible) && pv.tracked_refs == 0 {
                    if let Some(reason) = pv.touched.inapplicable_reason() {
                        return PinVerdict {
                            verdict: Verdict::Inapplicable { reason },
                            ..pv
                        };
                    }
                }
                pv
            })
            .collect();
        Self::from_results(results)
    }

    // 0 when every pin is Compatible or Inapplicable; 3 otherwise
    pub fn worst_exit_code(&self) -> i32 {
        if self.summary.is_blocking() {
            exit::NEEDS_ATTENTION
        } else {
            exit::OK
        }
    }
}

impl ContentPresence {
    /// Every considered hunk is already applied: the strong signal.
    pub fn fully_present(&self) -> bool {
        self.hunks_considered > 0 && self.hunks_already_applied == self.hunks_considered
    }
}

/// Advisory: the patch's content appears to already exist on the
/// target. Never affects the verdict; q-port and the operator own
/// the exclusion decision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AlreadyPresent {
    /// Tier-1 hit: an identical patch landed on the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identical: Option<TargetMatch>,
    /// Tier-2 tally: the post-image is (partly) present in the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentPresence>,
}

impl AlreadyPresent {
    pub fn is_empty(&self) -> bool {
        self.identical.is_none() && self.content.is_none()
    }
}

/// A dependency whose `rabbitmq-components.mk` pin differs between
/// the source commit's tree and the target tree, and that an app
/// touched by the patch (transitively) depends on. Advisory only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DepPinDivergence {
    pub dep: DependencyName,
    /// Pin at the source commit, e.g. `hex 2.16.0`.
    pub source: String,
    /// Pin at the target tree, e.g. `hex 2.13.0`.
    pub target: String,
}

/// A dep pin the candidate commit itself changes or introduces in the
/// root `rabbitmq-components.mk`. Advisory only: the verdict on a
/// bump-only diff stays `Inapplicable`, this is the evidence behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PinBump {
    pub dep: DependencyName,
    /// Pin before the commit, e.g. `hex 2.16.0`; `None` when the
    /// commit introduces the pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Pin after the commit, e.g. `hex 2.17.1`.
    pub to: String,
    /// Snapshot-store assessment of the bumped-to version. Detection
    /// is a pure function of the patch and may be cached; the store's
    /// contents are not a cache-key input, so this field is filled
    /// post-cache and cached values carry `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<BumpStatus>,
}

/// What the snapshot store knows about a bumped-to pin version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BumpStatus {
    /// The bumped dep is not a tracked project; nothing to vet.
    Untracked,
    /// No snapshot for the bumped-to version; `note` carries the remedy.
    SnapshotMissing {
        note: String,
    },
    SnapshotPresent,
}

/// Why already-present detection could not run. Emitted instead of
/// silence: an invisible skip would read as "not present".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum AlreadyPresentSkipped {
    /// The candidate commit object is absent from the target object
    /// store (an unfetched branch, usually).
    CandidateMissingFromTargetOdb { sha: CommitSha },
    /// The candidate and the target tip share no history.
    NoMergeBase,
    /// The target-side walk failed (shallow clone, corrupt object).
    TargetWalkFailed { detail: String },
}

/// Counts of call sites the analyzer could not resolve: `apply/3`-style
/// BIFs, and dispatch through a variable module or function. Informational only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Unanalyzed {
    #[serde(default)]
    pub apply: usize,
    #[serde(default)]
    pub variable_dispatch: usize,
}

impl Unanalyzed {
    pub fn is_empty(&self) -> bool {
        self.apply == 0 && self.variable_dispatch == 0
    }
}

fn summary_of(results: &[PinVerdict]) -> SeriesSummary {
    let mut summary = SeriesSummary::default();
    for r in results {
        match r.verdict {
            Verdict::Compatible => summary.compatible += 1,
            Verdict::RequiresAdaptation { .. } => summary.requires_adaptation += 1,
            Verdict::Incompatible { .. } => summary.incompatible += 1,
            Verdict::Inapplicable { .. } => summary.inapplicable += 1,
        }
    }
    summary
}

/// One symbol-availability question the post-cache probe answers:
/// "at which snapshot tag strictly later than this pin does the
/// symbol first appear?"
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AvailabilityQuery {
    ExactArity {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
    },
    AnyArity {
        module: ModuleName,
        function: FunctionName,
    },
    Record {
        name: RecordName,
    },
}

impl AvailabilityQuery {
    fn for_symbol(symbol: &SymbolRef) -> Option<Self> {
        match &symbol.kind {
            SymbolKind::Function { mfa } => Some(Self::ExactArity {
                module: mfa.module.clone(),
                function: mfa.function.clone(),
                arity: mfa.arity,
            }),
            SymbolKind::FunctionAnyArity { module, function } => Some(Self::AnyArity {
                module: module.clone(),
                function: function.clone(),
            }),
            SymbolKind::Record { name } => Some(Self::Record { name: name.clone() }),
            SymbolKind::Behaviour { .. }
            | SymbolKind::Callback { .. }
            | SymbolKind::Macro { .. }
            | SymbolKind::Type { .. } => None,
        }
    }
}

/// The output of `Patch::evaluate_series`: verdicts plus diagnostics plus
/// `PatchFacts` (source classifiers that drive downstream policy without
/// being verdict reasons).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SeriesEvaluation {
    pub verdict: SeriesVerdict,
    pub diagnostics: Diagnostics,
    #[serde(default, skip_serializing_if = "PatchFacts::is_empty")]
    pub patch_facts: PatchFacts,

    /// Paths the analysed patch touched, in diff-encounter order.
    /// Always emitted; an empty vec means the patch touched zero files.
    #[serde(default)]
    pub touched_paths: Vec<RelativePath>,

    /// Inner PR-branch commits for 2-parent merge SHAs. `None` for
    /// non-2-parent merges and non-merge SHAs.
    ///
    /// Do NOT add `skip_serializing_if = "Option::is_none"`. The
    /// `None` vs `Some(vec![])` distinction is wire-load-bearing.
    #[serde(default)]
    pub pr_commits: Option<Vec<PrCommit>>,

    /// Apply-axis prediction for this evaluation. `None` means no
    /// target context was supplied, so the axis was not evaluated:
    /// never "clean". A producer with a target context always emits
    /// `Some`, even when the patch touched zero files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply: Option<ApplyForecast>,

    /// Symbol-axis target findings for this evaluation. `None` means
    /// no target context was supplied, so the axis was not evaluated:
    /// never "clean". A producer with a target context always emits
    /// `Some`, even when no check produced a finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_findings: Option<TargetFindings>,
}

impl SeriesEvaluation {
    /// Folds all three axes: a blocking verdict, a predicted apply
    /// conflict, or a symbol finding on the target needs attention.
    /// Findings gate on non-empty, not blocking: the same reasons on
    /// an applicable pin read at least requires-adaptation.
    pub fn worst_exit_code(&self) -> i32 {
        if self.verdict.summary.is_blocking()
            || self.apply.as_ref().is_some_and(ApplyForecast::has_conflict)
            || self.target_findings.as_ref().is_some_and(|t| !t.is_empty())
        {
            exit::NEEDS_ATTENTION
        } else {
            exit::OK
        }
    }

    /// Populates symbol-availability fields on missing-symbol and
    /// arity-changed reasons, then reclassifies the affected pin
    /// verdicts (`is_blocking` is field-sensitive). `lookup` answers
    /// "earliest snapshot tag strictly later than this pin where the
    /// queried symbol exists", or `None` (including for self pins).
    /// Runs post-cache: the project's tag set is not a cache-key
    /// input, so cached values must stay unclassified.
    pub fn apply_symbol_availability<F>(&mut self, mut lookup: F)
    where
        F: FnMut(&Pin, &AvailabilityQuery) -> Option<TagName>,
    {
        let mut any_changed = false;
        for pv in &mut self.verdict.results {
            let pin = pv.pin.clone();
            let reasons = match &mut pv.verdict {
                Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
                    reasons
                }
                Verdict::Compatible | Verdict::Inapplicable { .. } => continue,
            };
            let mut changed = false;
            for reason in reasons.iter_mut() {
                match reason {
                    Reason::MissingSymbol {
                        symbol,
                        first_seen_at_tag: first_seen @ None,
                        needs_pin_at_least,
                        ..
                    } => {
                        let Some(query) = AvailabilityQuery::for_symbol(symbol) else {
                            continue;
                        };
                        if let Some(tag) = lookup(&pin, &query) {
                            *first_seen = Some(tag.clone());
                            *needs_pin_at_least = Some(NeedsPin {
                                project: pin.project.clone(),
                                tag,
                            });
                            changed = true;
                        }
                    }
                    Reason::ArityChanged {
                        module,
                        function,
                        expected,
                        expected_available_at: available @ None,
                        needs_pin_at_least,
                        ..
                    } => {
                        let query = AvailabilityQuery::ExactArity {
                            module: module.clone(),
                            function: function.clone(),
                            arity: *expected,
                        };
                        if let Some(tag) = lookup(&pin, &query) {
                            *available = Some(tag.clone());
                            *needs_pin_at_least = Some(NeedsPin {
                                project: pin.project.clone(),
                                tag,
                            });
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                let taken = mem::take(reasons);
                pv.verdict = Verdict::from_reasons(taken);
                any_changed = true;
            }
        }
        if any_changed {
            self.verdict.recompute_summary();
        }
    }
}

/// Source classifiers on the patch. Strictly separate from `Verdict` reasons.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PatchFacts {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub logging_style: BTreeMap<PathBuf, FileLoggingStyle>,
    #[serde(default, skip_serializing_if = "KhepriSignals::is_empty")]
    pub khepri_signals: KhepriSignals,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub introduces_versioned_record: BTreeSet<RecordName>,
}

impl PatchFacts {
    pub fn is_empty(&self) -> bool {
        self.logging_style.is_empty()
            && self.khepri_signals.is_empty()
            && self.introduces_versioned_record.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoggingStyle {
    LoggerMacros,
    RabbitLogModule,
    Mixed,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FileLoggingStyle {
    pub dominant: LoggingStyle,
    pub logger_macro_sites: usize,
    pub rabbit_log_sites: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KhepriSignals {
    #[serde(default)]
    pub touches_khepri_module: bool,
    #[serde(default)]
    pub uses_khepri_macros: bool,
    #[serde(default)]
    pub touches_only_khepri_branch: bool,
    #[serde(default)]
    pub touches_dual_branch: bool,
}

impl KhepriSignals {
    // `&self` is required by serde `skip_serializing_if`.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn is_empty(&self) -> bool {
        !self.touches_khepri_module
            && !self.uses_khepri_macros
            && !self.touches_only_khepri_branch
            && !self.touches_dual_branch
    }
}
