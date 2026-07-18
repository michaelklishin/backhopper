// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Diagnostics` and its tally types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{AlreadyPresent, AlreadyPresentSkipped, DepPinDivergence, PinBump, Unanalyzed};
use crate::model::names::{CommitSha, ModuleName, ProjectName, RecordName, RelativePath};

/// Series-wide diagnostic envelope. Strictly separate from `Verdict`
/// so untracked-call signals never leak into machine-readable verdicts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Diagnostics {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub untracked_calls: BTreeMap<ModuleName, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub untracked_records: BTreeMap<RecordName, usize>,
    /// In-scope references on *context* lines that do not resolve at
    /// some pin, per module. Pre-existing target facts: never reasons.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context_refs_missing: BTreeMap<ModuleName, usize>,
    /// Touched paths no configured project owns, tallied by their
    /// first two path components. The breadcrumb behind an
    /// `Inapplicable { Untracked }` verdict.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unattributed_paths: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Unanalyzed::is_empty")]
    pub unanalyzed: Unanalyzed,
    /// SUITE names worth running for the patch, derived from touched paths.
    /// Populated even when the verdict promotes to `Inapplicable` so that
    /// schema-only and config-only diffs still surface relevant suites.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_suites: Vec<String>,
    /// Per-`_SUITE.erl` map of `helper_module -> call_site_count`. The
    /// always-on diagnostic counterpart of
    /// `Reason::TestModuleSymbolMissing`: emitted by the parse pass
    /// even without a target tree (which is why the inner key is the
    /// `ModuleName` rather than a resolved-or-not bit). When
    /// `--target-repo-dir-path` is supplied the resolver re-walks this
    /// map and promotes the absent ones to typed reasons. The nested
    /// `BTreeMap` shape keeps the JSON envelope serialisable without
    /// a custom composite-key encoder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub missing_test_modules: BTreeMap<RelativePath, BTreeMap<ModuleName, usize>>,
    /// Advisory: the patch's content appears to already exist on the
    /// target (tier-1 identical patch, tier-2 content tally, or both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub already_present: Option<AlreadyPresent>,
    /// Why already-present detection was skipped, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub already_present_skipped: Option<AlreadyPresentSkipped>,
    /// Dep pins that diverge between source and target trees and are
    /// reachable from the apps this patch touches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dep_pin_divergence: Vec<DepPinDivergence>,
    /// Dep pins this patch itself changes or introduces in the root
    /// `rabbitmq-components.mk`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pin_bumps: Vec<PinBump>,
    /// What the live qualified-call gate's return-shape check did with
    /// each first-party resolved call: compared, or withheld and why.
    #[serde(default, skip_serializing_if = "ShapeCheckTally::is_empty")]
    pub qualified_call_shape_checks: ShapeCheckTally,
    /// The local-call twin: resolved unqualified calls, compared or
    /// withheld. Kept apart from the qualified tally so one axis's
    /// coverage never reads as the other's.
    #[serde(default, skip_serializing_if = "ShapeCheckTally::is_empty")]
    pub local_call_shape_checks: ShapeCheckTally,
    /// Same-file `-define` value comparisons over macros the patch
    /// uses: compared, or withheld and why.
    #[serde(default, skip_serializing_if = "MacroValueTally::is_empty")]
    pub macro_value_checks: MacroValueTally,
    /// What the indirect-reference check (meck expectations and rpc
    /// forms) did with each recognized occurrence: resolved against the
    /// target module, or seen with an unreadable arity and withheld.
    #[serde(default, skip_serializing_if = "IndirectCallTally::is_empty")]
    pub indirect_call_checks: IndirectCallTally,
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        // Destructure so adding a field forces this method to be updated.
        let Self {
            untracked_calls,
            untracked_records,
            context_refs_missing,
            unattributed_paths,
            unanalyzed,
            suggested_suites,
            missing_test_modules,
            already_present,
            already_present_skipped,
            dep_pin_divergence,
            pin_bumps,
            qualified_call_shape_checks,
            local_call_shape_checks,
            macro_value_checks,
            indirect_call_checks,
        } = self;
        untracked_calls.is_empty()
            && untracked_records.is_empty()
            && context_refs_missing.is_empty()
            && unattributed_paths.is_empty()
            && unanalyzed.is_empty()
            && suggested_suites.is_empty()
            && missing_test_modules.is_empty()
            && already_present.is_none()
            && already_present_skipped.is_none()
            && dep_pin_divergence.is_empty()
            && pin_bumps.is_empty()
            && qualified_call_shape_checks.is_empty()
            && local_call_shape_checks.is_empty()
            && macro_value_checks.is_empty()
            && indirect_call_checks.is_empty()
    }

    /// Record that `suite` references `helper`; bumps the call-site
    /// counter. Convenience for the parse pass.
    pub fn record_missing_test_module(&mut self, suite: RelativePath, helper: ModuleName) {
        let inner = self.missing_test_modules.entry(suite).or_default();
        *inner.entry(helper).or_insert(0) += 1;
    }
}

/// How a tier-1 already-present match against the target branch was
/// established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TargetMatchKind {
    /// A walked target commit's `-x` trailer names the candidate (or
    /// one of its trailer origins or inner PR commits).
    TrailerIntersection,
    /// One of the candidate's own trailer origins is already an
    /// ancestor of the target tip.
    TrailerAncestry,
    /// A walked target commit's normalized patch hash equals the
    /// candidate's.
    PatchId,
}

/// A target commit found to carry the candidate's patch. The subject
/// rides along so consumers can auto-fill exclusion reasons without
/// a second git query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TargetMatch {
    pub commit: CommitSha,
    pub via: TargetMatchKind,
    pub subject: String,
}

/// What the live qualified-call gate's return-shape check did with
/// each first-party resolved call. Never reasons: a withhold is not a
/// finding, but a round where most calls are shape-blind must read
/// differently from a round where all of them were compared.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ShapeCheckTally {
    /// Both sides had a `-spec` that parsed to a known type; the shapes
    /// were compared. Drift is visible in the reasons, not double-counted
    /// here.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub compared: usize,
    /// One or both sides declare no `-spec` for the called function.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_no_spec: usize,
    /// Both sides have a spec but at least one parses to
    /// `SpecType::Unknown`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_unknown_type: usize,
    /// The callee module was not readable on the source side: no
    /// checkout was supplied, or the checkout has no file at the
    /// module's path (a defaulted `--repo-dir-path` that is not the
    /// source tree lands here, not in `withheld_no_spec`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_no_source: usize,
    /// The call resolved through an `-import`: the callee's `-spec`
    /// lives in another module, so the calling module's tables cannot
    /// compare it. Local-call axis only.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_imported: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl ShapeCheckTally {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Fold another tally in, so the Erlang and Elixir extraction
    /// streams sum into one counter.
    pub fn merge(&mut self, other: Self) {
        self.compared += other.compared;
        self.withheld_no_spec += other.withheld_no_spec;
        self.withheld_unknown_type += other.withheld_unknown_type;
        self.withheld_no_source += other.withheld_no_source;
        self.withheld_imported += other.withheld_imported;
    }
}

/// What the macro-value check did with each `?MACRO` the patch uses:
/// compared, or withheld and why. Same-file `-define`s only; anything
/// reached another way is withheld, never guessed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MacroValueTally {
    /// Defined exactly once same-file on both sides; values compared.
    /// Drift is visible in the reasons, not double-counted here.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub compared: usize,
    /// Not defined same-file on both sides: the definition lives in an
    /// include, or moved between files across the trees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_definition_elsewhere: usize,
    /// Defined more than once in the file on either side (`-ifdef`
    /// branches): either value could be live.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_multiple_defines: usize,
    /// The touched file was not readable on the source side.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_no_source: usize,
}

impl MacroValueTally {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Fold another tally in, so the Erlang and Elixir extraction
    /// streams sum into one counter.
    pub fn merge(&mut self, other: Self) {
        self.compared += other.compared;
        self.withheld_definition_elsewhere += other.withheld_definition_elsewhere;
        self.withheld_multiple_defines += other.withheld_multiple_defines;
        self.withheld_no_source += other.withheld_no_source;
    }
}

/// What the indirect-reference check did with each recognized form
/// occurrence. `checked` counts deduplicated resolved sites, the same
/// dedup the reasons use; flagged sites are visible in the reasons,
/// not double-counted here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct IndirectCallTally {
    /// Module first-party, export and definition sets readable: the
    /// membership check ran.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub checked: usize,
    /// Module and function were literal atoms but the arity source was
    /// not statically readable: a variable argument list, a list with a
    /// cons tail, or a non-literal arity argument.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub withheld_dynamic: usize,
}

impl IndirectCallTally {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Fold another tally in, so the Erlang and Elixir extraction
    /// streams sum into one counter.
    pub fn merge(&mut self, other: Self) {
        self.checked += other.checked;
        self.withheld_dynamic += other.withheld_dynamic;
    }
}

/// Per-file hunk counters for the content-presence check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HunkTally {
    pub applied: usize,
    pub considered: usize,
    pub ambiguous: usize,
    pub low_confidence: usize,
}

/// Tier-2 advisory: how much of the patch's post-image already exists
/// in the named pin's tree. A tally, never a boolean: partial presence
/// is common and the fraction is the signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ContentPresence {
    /// The pin whose evaluation produced the tally. In cross-branch
    /// mode the self pin resolves to the target branch, so its tally
    /// is the target-tree answer.
    pub pin: ProjectName,
    pub hunks_already_applied: usize,
    pub hunks_considered: usize,
    pub hunks_ambiguous: usize,
    pub hunks_low_confidence: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_file: BTreeMap<RelativePath, HunkTally>,
}
