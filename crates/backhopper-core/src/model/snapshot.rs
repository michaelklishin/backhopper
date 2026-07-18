// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Snapshot<S>` with the type-state pattern.
//!
//! The two states `Unsorted` and `Canonical` enforce that:
//!  * the on-disk format writer accepts only `Snapshot<Canonical>`
//!  * the parser produces `Snapshot<Canonical>` and rejects any
//!    non-canonical input

use std::collections::BTreeMap;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::compat::arg_shape::ArgShape;
use crate::model::names::{
    ApplicationName, Arity, CommitSha, DependencyName, DependencyVersion, FieldName, FunctionName,
    MacroName, ModuleName, ProjectName, RecordName, TagName, TypeName,
};
use crate::snapshot::sort::canonicalize;

pub mod state {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Unsorted;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Canonical;
}

pub const FORMAT_VERSION: u32 = 4;

/// Older snapshot file versions the parser still accepts. New writers
/// always emit `FORMAT_VERSION`. `snapshots migrate` regenerates each
/// stored snapshot at the current version.
pub const SUPPORTED_FORMAT_VERSIONS: &[u32] = &[1, 2, 3, 4];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotHeader {
    pub project: ProjectName,
    pub tag: TagName,
    pub branch: Option<String>,
    pub commit: CommitSha,
    pub scanned_paths: Vec<String>,
    /// Names of Erlang/OTP applications included in this snapshot. Populated
    /// for `multi_app` and `erlang_otp` projects: empty for `single_app`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apps_scanned: Vec<ApplicationName>,
    pub generated_by: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    /// Version of the extractor that produced this snapshot. Compared against
    /// the running binary's `EXTRACTOR_VERSION` by `snapshots verify --all`.
    /// Empty string when reading pre-extractor-versioning snapshots.
    #[serde(default)]
    pub extractor_version: String,
    /// Vendored dependency pins captured from the project's components file
    /// (e.g. `rabbitmq-components.mk`). Populated for projects whose family
    /// declares a components-file shape; empty otherwise. Format version 4
    /// and later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dep_pins: Vec<VendoredDep>,
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

    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "hidden" => Some(Self::Hidden),
            "test_only" => Some(Self::TestOnly),
            _ => None,
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
    /// Per-export clause-head patterns: the disjunction of clauses that
    /// define this function. Used by the analyzer to flag calls whose
    /// argument shape doesn't satisfy any clause head at the pin.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clause_heads: BTreeMap<FunArity, Vec<Vec<ArgShape>>>,
    /// Source path relative to the project root. `None` on format
    /// version 1 snapshots; populated on v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Application this module belongs to. `None` on v1 and for
    /// single-app projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<ApplicationName>,
    /// Records declared inline in the `.erl` file. Distinct from
    /// `HrlFile.records`. v2 and later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<RecordDecl>,
    /// Exports declared inside an `-ifdef(TEST)` block. Test-only
    /// exports drive cascade planning for the unconditional-exports
    /// refactor: they are not part of the public API at the tag but the
    /// planner needs them recorded for this branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_only_exports: Vec<TestOnlyExport>,
    /// `-define(...)` macros inside `-ifdef(TEST)` *body*
    /// blocks (the macro itself is only visible under the guard).
    /// Needed by the Variant B unwrap planner: when the body block
    /// becomes unconditional, every macro it depends on must move
    /// out together with the function bodies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ifdef_macros: Vec<IfdefMacro>,
    /// `-ifndef(TEST)/-else/-endif` regions: a function with two
    /// bodies, a production one and a test one. Recorded for
    /// surfacing; no verdict rule fires on them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variant_c_blocks: Vec<VariantCBlock>,
    /// Extracted body of the version function for modules that implement
    /// a `versioned_machine_impl` declared by the project family (e.g.
    /// `rabbit_fifo:version/0`). `None` when the module is not a declared
    /// versioned-machine impl or when the body did not resolve. Format
    /// version 4 and later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versioned_machine_version: Option<VersionedMachineVersion>,
    /// Bound values of the wire-bearing macros declared by the project
    /// family for this module (e.g. `RA_PROTO_VERSION` on `ra`). Empty
    /// when the module declares no tracked wire constants. Format
    /// version 4 and later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wire_constants: Vec<WireConstantBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TestOnlyExport {
    pub function: FunctionName,
    pub arity: Arity,
    /// 1-based line of the `-export` directive.
    pub export_line: usize,
    /// 1-based line where the function body starts. `None` for
    /// Variant A (body unconditional), `Some` for Variant B (body
    /// inside its own `-ifdef(TEST)` block, where the function
    /// itself is absent from a non-TEST beam).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_line: Option<usize>,
    pub variant: TestExportVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestExportVariant {
    /// Body is compiled unconditionally; only the `-export` is
    /// guarded. Stale non-TEST `.beam` still has the function code
    /// but not in its export table.
    A,
    /// Body is also inside an `-ifdef(TEST)` block. The function
    /// does not exist in a non-TEST `.beam` at all.
    B,
}

impl TestExportVariant {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IfdefMacro {
    pub name: String,
    /// 1-based line of the `-define` directive.
    pub line: usize,
    pub guard_kind: IfdefGuardKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfdefGuardKind {
    /// The macro is inside `-ifdef(TEST)`.
    Test,
    /// The macro is inside `-ifndef(TEST)`.
    NotTest,
    /// The macro is inside some other `-ifdef` or `-if` block.
    Other,
}

impl IfdefGuardKind {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::NotTest => "not_test",
            Self::Other => "other",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "test" => Some(Self::Test),
            "not_test" => Some(Self::NotTest),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VariantCBlock {
    /// The `-ifndef(...)` identifier (typically `TEST`).
    pub guard: String,
    /// 1-based line of the `-ifndef` directive.
    pub start_line: usize,
    /// 1-based line of the matching `-endif.`.
    pub end_line: usize,
    /// 1-based line of the `-else.`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub else_line: Option<usize>,
}

/// Snapshot-level record of a versioned state-machine's `version/0`
/// function value, captured from the source at snapshot time. Populated
/// only for modules the project family declares as versioned-machine
/// impls (e.g. `rabbit_fifo`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VersionedMachineVersion {
    pub function: FunctionName,
    pub arity: Arity,
    /// Resolved integer value of the version-function body. `None` when
    /// the body did not parse to an integer literal (directly or through
    /// macro expansion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<u64>,
    pub provenance: Provenance,
}

/// How a versioned-machine `version/0` value was resolved at snapshot
/// time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Provenance {
    /// The body was an integer literal directly in the function clause.
    Literal,
    /// The body referenced a `?MACRO` that resolved to an integer.
    /// `defined_in` is the file path of the source that bound the macro
    /// (own `.erl`, own `.hrl`, or a transitive include).
    MacroBody {
        macro_name: MacroName,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defined_in: Option<String>,
    },
}

/// Snapshot-level record of a wire-bearing macro's bound value (e.g.
/// `RA_PROTO_VERSION`, segment-file `VERSION` and `MAGIC`). Captured
/// per module declared by the project family as wire-bearing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WireConstantBinding {
    pub macro_name: MacroName,
    pub value: WireValue,
    /// Source file where the macro was `-define`'d. May be the module's
    /// own `.erl`, a sibling `.hrl`, or a transitive include.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defined_in: Option<String>,
}

/// Type-discriminated wire-constant value. Integers and ASCII bytestrings
/// are typed; anything else is preserved verbatim as `Opaque`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WireValue {
    /// Unsigned integer literal (`-define(VERSION, 2)`).
    U64(u64),
    /// Bytestring literal (`-define(MAGIC, <<"RASG">>)`). Stored as
    /// the inner bytes without the `<<"...">>` wrapper. The text-format
    /// round-trip assumes ASCII-printable bytes: non-printable byte
    /// content should land in `Opaque` instead.
    Bytes(Vec<u8>),
    /// Anything else, preserved as the raw token text.
    Opaque(String),
}

/// Pinned dependency captured from the project's components file (e.g.
/// `rabbitmq-components.mk`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VendoredDep {
    pub name: DependencyName,
    pub version: DependencyVersion,
    pub source: VendoredDepSource,
}

/// How a vendored dependency is sourced. Mirrors the three real
/// `rabbitmq-components.mk` line shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VendoredDepSource {
    /// `dep_NAME = hex VERSION`.
    Hex,
    /// `dep_NAME = git URL TAG`.
    Git,
    /// `dep_NAME = git_rmq NAME TAG` (vendored fork).
    GitRmq,
}

impl VendoredDepSource {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Git => "git",
            Self::GitRmq => "git_rmq",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "hex" => Some(Self::Hex),
            "git" => Some(Self::Git),
            "git_rmq" => Some(Self::GitRmq),
            _ => None,
        }
    }
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
            clause_heads: BTreeMap::new(),
            path: None,
            app: None,
            records: Vec::new(),
            test_only_exports: Vec::new(),
            ifdef_macros: Vec::new(),
            variant_c_blocks: Vec::new(),
            versioned_machine_version: None,
            wire_constants: Vec::new(),
        }
    }

    /// Exports that have no matching clause head. An empty
    /// `clause_heads` (older snapshots without the field populated)
    /// returns an empty list so this never false-fires on legacy data.
    pub fn missing_clause_bodies(&self) -> Vec<FunArity> {
        if self.clause_heads.is_empty() {
            return Vec::new();
        }
        self.exports
            .iter()
            .filter(|fa| !self.clause_heads.contains_key(fa))
            .cloned()
            .collect()
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    #[must_use]
    pub fn into_canonical(mut self) -> Snapshot<state::Canonical> {
        canonicalize(&mut self.modules, &mut self.headers);
        self.header.apps_scanned.sort();
        self.header.apps_scanned.dedup();
        self.header.dep_pins.sort_by(|a, b| a.name.cmp(&b.name));
        self.header.dep_pins.dedup_by(|a, b| a.name == b.name);
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

    /// Records are declared in headers (.hrl) and inline in modules (.erl): both.
    pub fn records(&self) -> impl Iterator<Item = &RecordDecl> {
        self.headers
            .iter()
            .flat_map(|h| h.records.iter())
            .chain(self.modules.iter().flat_map(|m| m.records.iter()))
    }
}

impl Snapshot<state::Canonical> {
    /// True when `module` exports `function/arity`. O(log n): canonical
    /// snapshots keep `modules` and each module's `exports` sorted.
    pub fn lookup_export(
        &self,
        module: &ModuleName,
        function: &FunctionName,
        arity: Arity,
    ) -> bool {
        let Some(m) = self.module_named(module) else {
            return false;
        };
        let target = FunArity {
            name: function.clone(),
            arity,
        };
        m.exports.binary_search(&target).is_ok()
    }

    /// Look up a module by name in O(log n): canonical snapshots keep
    /// `modules` sorted by `name`.
    pub fn module_named(&self, name: &ModuleName) -> Option<&Module> {
        self.modules
            .binary_search_by(|m| m.name.cmp(name))
            .ok()
            .map(|idx| &self.modules[idx])
    }

    /// Every module whose `-behaviour(B).` list contains `behaviour`.
    /// Reverse index over `Module.behaviours`; built per call (one
    /// linear scan over `self.modules`). Used by behaviour-conformance
    /// checks and the R7 suite-implementer sweep.
    pub fn implementers_of(&self, behaviour: &ModuleName) -> Vec<&Module> {
        self.modules
            .iter()
            .filter(|m| m.behaviours.contains(behaviour))
            .collect()
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
                apps_scanned: Vec::new(),
                generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
                generated_at: OffsetDateTime::UNIX_EPOCH,
                extractor_version: String::new(),
                dep_pins: Vec::new(),
            },
            modules: Vec::new(),
            headers: Vec::new(),
            _state: PhantomData,
        }
    }
}
