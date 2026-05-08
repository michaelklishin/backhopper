//! `Snapshot<S>` with the type-state pattern.
//!
//! The two states `Unsorted` and `Canonical` enforce that:
//!  * the on-disk format writer accepts only `Snapshot<Canonical>`
//!  * the parser produces `Snapshot<Canonical>` and rejects any
//!    non-canonical input

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
    TypeName,
};

pub mod state {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Unsorted;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Canonical;
}

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotHeader {
    pub project: ProjectName,
    pub tag: TagName,
    pub branch: Option<String>,
    pub commit: CommitSha,
    pub scanned_paths: Vec<String>,
    pub generated_by: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Hidden,
    TestOnly,
}

impl Visibility {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Hidden => "hidden",
            Self::TestOnly => "test_only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Module {
    pub name: ModuleName,
    pub visibility: Visibility,
    pub behaviours: Vec<ModuleName>,
    pub exports: Vec<FunArity>,
    pub export_types: Vec<TypeArity>,
    pub callbacks: Vec<CallbackSig>,
    pub optional_callbacks: Vec<FunArity>,
    pub specs: Vec<SpecSig>,
    pub types: Vec<TypeDecl>,
    pub opaques: Vec<TypeArity>,
    pub deprecations: Vec<Deprecation>,
}

impl Module {
    pub fn new(name: ModuleName) -> Self {
        Self {
            name,
            visibility: Visibility::Public,
            behaviours: Vec::new(),
            exports: Vec::new(),
            export_types: Vec::new(),
            callbacks: Vec::new(),
            optional_callbacks: Vec::new(),
            specs: Vec::new(),
            types: Vec::new(),
            opaques: Vec::new(),
            deprecations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrlFile {
    pub path: String,
    pub types: Vec<TypeDecl>,
    pub opaques: Vec<TypeArity>,
    pub records: Vec<RecordDecl>,
}

impl HrlFile {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            types: Vec::new(),
            opaques: Vec::new(),
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunArity {
    pub name: FunctionName,
    pub arity: Arity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeArity {
    pub name: TypeName,
    pub arity: Arity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallbackSig {
    pub name: FunctionName,
    pub arity: Arity,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecSig {
    pub name: FunctionName,
    pub arity: Arity,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeDecl {
    pub name: TypeName,
    pub arity: Arity,
    pub rhs: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordDecl {
    pub name: RecordName,
    pub fields: Vec<RecordField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordField {
    pub name: FieldName,
    pub type_repr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deprecation {
    pub function: Option<FunctionName>,
    pub arity_match: ArityMatch,
    pub since: Option<TagName>,
    pub replacement: Option<DeprecationReplacement>,
    pub reason: Option<String>,
    pub module_wide: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ArityMatch {
    Exact { arity: Arity },
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeprecationReplacement {
    pub function: FunctionName,
    pub arity: Arity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Snapshot<S = state::Unsorted> {
    pub header: SnapshotHeader,
    pub modules: Vec<Module>,
    pub headers: Vec<HrlFile>,
    #[serde(skip)]
    _state: PhantomData<S>,
}

impl Snapshot<state::Unsorted> {
    pub fn from_extracted(
        header: SnapshotHeader,
        modules: Vec<Module>,
        headers: Vec<HrlFile>,
    ) -> Self {
        Self {
            header,
            modules,
            headers,
            _state: PhantomData,
        }
    }

    pub fn into_canonical(mut self) -> Snapshot<state::Canonical> {
        crate::snapshot::sort::canonicalize(&mut self.modules, &mut self.headers);
        Snapshot {
            header: self.header,
            modules: self.modules,
            headers: self.headers,
            _state: PhantomData,
        }
    }
}

impl<S> Snapshot<S> {
    pub fn header(&self) -> &SnapshotHeader {
        &self.header
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    pub fn headers(&self) -> &[HrlFile] {
        &self.headers
    }
}

impl Snapshot<state::Canonical> {
    pub fn lookup_export(
        &self,
        module: &ModuleName,
        function: &FunctionName,
        arity: Arity,
    ) -> bool {
        self.modules
            .iter()
            .find(|m| &m.name == module)
            .map(|m| {
                m.exports
                    .iter()
                    .any(|fa| &fa.name == function && fa.arity == arity)
            })
            .unwrap_or(false)
    }

    pub(crate) fn from_canonical_parts(
        header: SnapshotHeader,
        modules: Vec<Module>,
        headers: Vec<HrlFile>,
    ) -> Self {
        Self {
            header,
            modules,
            headers,
            _state: PhantomData,
        }
    }
}

impl Default for Snapshot<state::Unsorted> {
    fn default() -> Self {
        Self {
            header: SnapshotHeader {
                project: ProjectName::new("placeholder").expect("valid"),
                tag: TagName::new("v0.0.0").expect("valid"),
                branch: None,
                commit: CommitSha::new("0".repeat(40)).expect("valid"),
                scanned_paths: Vec::new(),
                generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
            modules: Vec::new(),
            headers: Vec::new(),
            _state: PhantomData,
        }
    }
}
