// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::compat::arg_shape::ArgShape;
use crate::model::names::{
    Arity, CommitSha, FieldName, FunctionName, GitRef, ModuleName, RecordName, TagName,
};
use crate::model::pin::Pin;
use crate::model::symbol::SymbolRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Compatible,
    RequiresAdaptation { reasons: Vec<Reason> },
    Incompatible { reasons: Vec<Reason> },
    Inapplicable { reason: InapplicableReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum InapplicableReason {
    NoErlangSurfaceTouched,
    OnlyDocsTouched,
    OnlyTestFixturesTouched,
    OnlySchemaTouched,
}

impl InapplicableReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoErlangSurfaceTouched => "no_erlang_surface_touched",
            Self::OnlyDocsTouched => "only_docs_touched",
            Self::OnlyTestFixturesTouched => "only_test_fixtures_touched",
            Self::OnlySchemaTouched => "only_schema_touched",
        }
    }
}

impl Verdict {
    pub fn from_reasons(reasons: Vec<Reason>) -> Self {
        if reasons.is_empty() {
            return Self::Compatible;
        }
        if reasons.iter().any(Reason::is_blocking) {
            Self::Incompatible { reasons }
        } else {
            Self::RequiresAdaptation { reasons }
        }
    }

    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }

    pub fn reasons(&self) -> &[Reason] {
        match self {
            Self::Compatible | Self::Inapplicable { .. } => &[],
            Self::RequiresAdaptation { reasons } | Self::Incompatible { reasons } => reasons,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    MissingSymbol {
        symbol: SymbolRef,
        first_seen_at_tag: Option<TagName>,
        suggested_replacement: Option<SymbolRef>,
    },
    ArityChanged {
        module: ModuleName,
        function: FunctionName,
        expected: Arity,
        found: Vec<Arity>,
    },
    SignatureChanged {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        expected_spec: String,
        found_spec: String,
    },
    FileAbsent {
        path: PathBuf,
    },
    ContextDrift {
        path: PathBuf,
        hunk_index: usize,
    },
    DeprecatedUsage {
        symbol: SymbolRef,
        since: Option<TagName>,
        replacement: Option<SymbolRef>,
    },
    NowHidden {
        module: ModuleName,
    },
    RecordFieldsChanged {
        record: RecordName,
        expected: Vec<FieldName>,
        found: Vec<FieldName>,
    },
    UnsupportedFileType {
        path: PathBuf,
    },
    UntrackedModuleMissing {
        module: ModuleName,
    },
    /// Call-site argument shapes don't satisfy any clause head at the
    /// pin. Emitted when both the call and the pin's clause patterns
    /// are concrete enough to compare; `Unknown` on either side is the
    /// escape hatch that suppresses the reason.
    ClauseMismatch {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
        call_args: Vec<ArgShape>,
        pin_clauses: Vec<Vec<ArgShape>>,
    },
    /// Patch references an MFA owned by the self-project but absent from the
    /// resolved self-snapshot. `suggested_source_for_prereq` is filled only
    /// with `--suggest-prereqs`.
    MissingPrereq {
        symbol: SymbolRef,
        self_branch: GitRef,
        suggested_source_for_prereq: Option<CommitSha>,
    },
}

impl Reason {
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            Self::MissingSymbol { .. }
                | Self::ArityChanged { .. }
                | Self::SignatureChanged { .. }
                | Self::FileAbsent { .. }
                | Self::NowHidden { .. }
                | Self::RecordFieldsChanged { .. }
                | Self::UntrackedModuleMissing { .. }
                | Self::ClauseMismatch { .. }
                | Self::MissingPrereq { .. }
        )
    }
}

/// Per-pin tally of file kinds the patch touched. Lets `promote_inapplicable`
/// tell a real green check from a diff with nothing analyzable to check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchedKinds {
    #[serde(default)]
    pub erl: u32,
    #[serde(default)]
    pub hrl: u32,
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub docs: u32,
    #[serde(default)]
    pub tests: u32,
    #[serde(default)]
    pub other: u32,
}

impl TouchedKinds {
    pub fn is_empty(&self) -> bool {
        self.erl == 0
            && self.hrl == 0
            && self.schema == 0
            && self.docs == 0
            && self.tests == 0
            && self.other == 0
    }

    pub fn classify(path: &Path) -> FileKind {
        let p = path.to_string_lossy();
        let lower = p.to_ascii_lowercase();
        if lower.ends_with(".md")
            || lower.ends_with(".adoc")
            || lower.ends_with(".rst")
            || lower.ends_with(".txt")
            || lower.contains("/docs/")
            || lower.starts_with("docs/")
        {
            return FileKind::Docs;
        }
        if lower.contains("_suite_data/")
            || lower.contains("/test/")
            || lower.contains("/tests/")
            || lower.starts_with("test/")
            || lower.starts_with("tests/")
            || lower.ends_with("_suite.erl")
        {
            return FileKind::Tests;
        }
        if lower.ends_with(".schema") || lower.ends_with(".snippets") {
            return FileKind::Schema;
        }
        if lower.ends_with(".erl") {
            return FileKind::Erl;
        }
        if lower.ends_with(".hrl") {
            return FileKind::Hrl;
        }
        FileKind::Other
    }

    pub fn record(&mut self, kind: FileKind) {
        match kind {
            FileKind::Erl => self.erl += 1,
            FileKind::Hrl => self.hrl += 1,
            FileKind::Schema => self.schema += 1,
            FileKind::Docs => self.docs += 1,
            FileKind::Tests => self.tests += 1,
            FileKind::Other => self.other += 1,
        }
    }

    pub fn from_paths<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut tk = Self::default();
        for p in paths {
            tk.record(Self::classify(p.as_ref()));
        }
        tk
    }

    /// `None` when any `.erl` or `.hrl` was touched: an analyzable diff with
    /// zero reasons is real `Compatible`, not `Inapplicable`.
    pub fn inapplicable_reason(&self) -> Option<InapplicableReason> {
        if self.erl > 0 || self.hrl > 0 {
            return None;
        }
        if self.is_empty() {
            return Some(InapplicableReason::NoErlangSurfaceTouched);
        }
        let only_docs = self.docs > 0 && self.schema == 0 && self.tests == 0 && self.other == 0;
        if only_docs {
            return Some(InapplicableReason::OnlyDocsTouched);
        }
        let only_tests = self.tests > 0 && self.schema == 0 && self.docs == 0 && self.other == 0;
        if only_tests {
            return Some(InapplicableReason::OnlyTestFixturesTouched);
        }
        let only_schema = self.schema > 0 && self.docs == 0 && self.tests == 0 && self.other == 0;
        if only_schema {
            return Some(InapplicableReason::OnlySchemaTouched);
        }
        Some(InapplicableReason::NoErlangSurfaceTouched)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Erl,
    Hrl,
    Schema,
    Docs,
    Tests,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesVerdict {
    pub results: Vec<PinVerdict>,
    pub summary: SeriesSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesSummary {
    pub compatible: u32,
    pub requires_adaptation: u32,
    pub incompatible: u32,
    #[serde(default)]
    pub inapplicable: u32,
}

/// Process exit codes returned by `worst_exit_code`.
pub mod exit {
    pub const OK: i32 = 0;
    pub const INCOMPATIBLE: i32 = 2;
    pub const REQUIRES_ADAPTATION: i32 = 3;
    pub const INAPPLICABLE: i32 = 4;
}

impl SeriesVerdict {
    pub fn from_results(results: Vec<PinVerdict>) -> Self {
        let mut summary = SeriesSummary::default();
        for r in &results {
            match r.verdict {
                Verdict::Compatible => summary.compatible += 1,
                Verdict::RequiresAdaptation { .. } => summary.requires_adaptation += 1,
                Verdict::Incompatible { .. } => summary.incompatible += 1,
                Verdict::Inapplicable { .. } => summary.inapplicable += 1,
            }
        }
        Self { results, summary }
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

    /// Severity for the process exit code:
    /// `Incompatible` > `RequiresAdaptation` > `Compatible` > `Inapplicable`.
    /// A real `Compatible` beats `Inapplicable` because at least one pin produced
    /// a definitive green signal.
    pub fn worst_exit_code(&self) -> i32 {
        if self.summary.incompatible > 0 {
            exit::INCOMPATIBLE
        } else if self.summary.requires_adaptation > 0 {
            exit::REQUIRES_ADAPTATION
        } else if self.summary.compatible > 0 {
            exit::OK
        } else if self.summary.inapplicable > 0 {
            exit::INAPPLICABLE
        } else {
            exit::OK
        }
    }
}

/// Series-wide diagnostic envelope. Strictly separate from `Verdict`
/// so untracked-call signals never leak into machine-readable verdicts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostics {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub untracked_calls: BTreeMap<ModuleName, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub untracked_records: BTreeMap<RecordName, usize>,
    #[serde(default, skip_serializing_if = "Unanalyzed::is_empty")]
    pub unanalyzed: Unanalyzed,
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        self.untracked_calls.is_empty()
            && self.untracked_records.is_empty()
            && self.unanalyzed.is_empty()
    }
}

/// Counts of call sites the analyzer could not resolve: `apply/3`-style
/// BIFs, and dispatch through a variable module or function. Informational only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// The output of `Patch::evaluate_series`: verdicts plus diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesEvaluation {
    pub verdict: SeriesVerdict,
    pub diagnostics: Diagnostics,
}

impl SeriesEvaluation {
    pub fn worst_exit_code(&self) -> i32 {
        self.verdict.worst_exit_code()
    }
}
