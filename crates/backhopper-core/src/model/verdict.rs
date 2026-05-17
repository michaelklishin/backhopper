use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::compat::arg_shape::ArgShape;
use crate::model::names::{Arity, FieldName, FunctionName, ModuleName, RecordName, TagName};
use crate::model::pin::Pin;
use crate::model::symbol::SymbolRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Compatible,
    RequiresAdaptation { reasons: Vec<Reason> },
    Incompatible { reasons: Vec<Reason> },
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
            Self::Compatible => &[],
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
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinVerdict {
    pub pin: Pin,
    pub verdict: Verdict,
    #[serde(default)]
    pub tracked_refs: usize,
}

impl PinVerdict {
    pub fn new(pin: Pin, verdict: Verdict) -> Self {
        Self {
            pin,
            verdict,
            tracked_refs: 0,
        }
    }

    #[must_use]
    pub fn with_tracked_refs(mut self, n: usize) -> Self {
        self.tracked_refs = n;
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
}

impl SeriesVerdict {
    pub fn from_results(results: Vec<PinVerdict>) -> Self {
        let mut summary = SeriesSummary::default();
        for r in &results {
            match r.verdict {
                Verdict::Compatible => summary.compatible += 1,
                Verdict::RequiresAdaptation { .. } => summary.requires_adaptation += 1,
                Verdict::Incompatible { .. } => summary.incompatible += 1,
            }
        }
        Self { results, summary }
    }

    pub fn worst_exit_code(&self) -> i32 {
        if self.summary.incompatible > 0 { 1 } else { 0 }
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
