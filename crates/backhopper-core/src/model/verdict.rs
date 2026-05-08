use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinVerdict {
    pub pin: Pin,
    pub verdict: Verdict,
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
