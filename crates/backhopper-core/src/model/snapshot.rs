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
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::compat::arg_shape::ArgShape;
use crate::errors::{NameError, SnapshotError};
use crate::model::app_manifest::AppManifest;
use crate::model::macro_decl::MacroDecl;
use crate::model::names::{
    ApplicationName, Arity, CommitSha, DependencyName, DependencyVersion, ExtractionSettingsDigest,
    FieldName, FunctionName, HostFactsDigest, MacroName, ModuleName, ProjectName, RecordName,
    RelativePath, TagName, TypeName,
};
use crate::model::non_empty::NonEmpty;
use crate::model::normalised::NormalisedDefault;
use crate::model::schema_files::{PartialFile, SchemaFile};
use crate::snapshot::sort::{canonicalize_parts, missing_app, out_of_order};

pub mod state {
    mod private {
        pub trait Sealed {}
    }

    /// Sealed so a foreign marker cannot stand in for `Unsorted` or
    /// `Canonical` and skip the canonicalization the two states pin.
    pub trait SnapshotState: private::Sealed {}

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Unsorted;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Canonical;

    impl private::Sealed for Unsorted {}
    impl private::Sealed for Canonical {}
    impl SnapshotState for Unsorted {}
    impl SnapshotState for Canonical {}
}

pub const FORMAT_VERSION: u32 = 5;

/// Format 5 adds facts a re-emitted older snapshot does not hold, so only
/// regeneration writes it.
pub const LAST_MIGRATABLE_FORMAT_VERSION: u32 = 4;

pub const SUPPORTED_FORMAT_VERSIONS: &[u32] = &[1, 2, 3, 4, 5];

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
    /// The file's `format-version` header, as parsed. Defaults to 0 when
    /// deserializing an envelope written before this field existed.
    #[serde(default)]
    pub format_version: u32,
    /// Vendored dependency pins captured from the project's components file
    /// (e.g. `rabbitmq-components.mk`). Populated for projects whose family
    /// declares a components-file shape; empty otherwise. Format version 4
    /// and later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dep_pins: Vec<VendoredDep>,
    /// The release roots and configuration digest a host snapshot was
    /// generated with. `None` for every other project. Format version 5
    /// and later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_facts: Option<HostReleaseFacts>,
    /// Digest of the project settings the snapshot was extracted with.
    /// `None` for a file that records none. Format version 5 and later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_settings: Option<ExtractionSettingsDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostReleaseFacts {
    pub release_roots: NonEmpty<ApplicationName>,
    pub digest: HostFactsDigest,
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
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "crate::model::serde_util::fun_arity_keyed_map"
    )]
    pub clause_heads: BTreeMap<FunArity, Vec<Vec<ArgShape>>>,
    /// Source path relative to the project root. `None` on format
    /// version 1 snapshots; populated on v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<RelativePath>,
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

    /// Whether the module exports `function/arity`. The export list is
    /// sorted in a canonical snapshot.
    pub fn exports_function(&self, function: &FunctionName, arity: Arity) -> bool {
        let key = FunArity {
            name: function.clone(),
            arity,
        };
        self.exports.binary_search(&key).is_ok()
    }

    /// Whether the module lists `name/arity` in `-export_type`.
    pub fn exports_type(&self, name: &TypeName, arity: Arity) -> bool {
        self.export_types.contains(&TypeArity {
            name: name.clone(),
            arity,
        })
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HeaderRef {
    pub app: ApplicationName,
    pub app_path: RelativePath,
}

impl fmt::Display for HeaderRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.app, self.app_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrlFile {
    pub path: RelativePath,
    /// Application the header belongs to. `None` before format version 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<ApplicationName>,
    /// The header's own `-include` and `-include_lib` lines. Format
    /// version 5 and later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<HeaderRef>,
    pub types: Vec<TypeDecl>,
    pub opaques: Vec<TypeArity>,
    pub records: Vec<RecordDecl>,
    /// Macros the header defines outside `-ifdef(TEST)`. Format version 5
    /// and later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defines: Vec<MacroDecl>,
}

impl HrlFile {
    pub fn new(path: RelativePath) -> Self {
        Self {
            path,
            app: None,
            includes: Vec::new(),
            types: Vec::new(),
            opaques: Vec::new(),
            records: Vec::new(),
            defines: Vec::new(),
        }
    }

    /// The part of `path` after the app root, which is the directory named
    /// like the app: `deps/rabbit_common/include/rabbit.hrl` in
    /// `rabbit_common` is `include/rabbit.hrl`. A path with no such
    /// directory is already relative to the app. `None` without an app.
    #[must_use]
    pub fn app_path(&self) -> Option<RelativePath> {
        let app = self.app.as_ref()?;
        let segments: Vec<&str> = self.path.as_str().split('/').collect();
        let (_file, dirs) = segments.split_last()?;
        let rest = match dirs.iter().position(|d| *d == app.as_str()) {
            Some(root) => segments[root + 1..].join("/"),
            None => self.path.as_str().to_owned(),
        };
        RelativePath::new(rest).ok()
    }

    #[must_use]
    pub fn header_ref(&self) -> Option<HeaderRef> {
        Some(HeaderRef {
            app: self.app.clone()?,
            app_path: self.app_path()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FunArity {
    pub name: FunctionName,
    pub arity: Arity,
}

impl fmt::Display for FunArity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.name, self.arity)
    }
}

impl FromStr for FunArity {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        let (name, arity) = split_name_arity("fun_arity", s)?;
        Ok(Self {
            name: name.parse()?,
            arity,
        })
    }
}

/// The `name/arity` split `FunArity` and `TypeArity` share; the name half
/// is returned unparsed because the two kinds validate it differently.
fn split_name_arity<'a>(kind: &'static str, s: &'a str) -> Result<(&'a str, Arity), NameError> {
    let invalid = || NameError::Invalid {
        kind,
        value: s.to_owned(),
        reason: "expected 'name/arity'",
    };
    let (name, arity) = s.rsplit_once('/').ok_or_else(invalid)?;
    let arity = arity.parse().map_err(|_| invalid())?;
    Ok((name, arity))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypeArity {
    pub name: TypeName,
    pub arity: Arity,
}

impl fmt::Display for TypeArity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.name, self.arity)
    }
}

impl FromStr for TypeArity {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        let (name, arity) = split_name_arity("type_arity", s)?;
        Ok(Self {
            name: name.parse()?,
            arity,
        })
    }
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
    /// `None` means the source wrote no default, which Erlang reads as
    /// `undefined`; a written `= undefined` is `Some`. Format version 5
    /// and later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<NormalisedDefault>,
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
pub(crate) struct SnapshotParts {
    pub(crate) header: SnapshotHeader,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) apps: Vec<AppManifest>,
    pub(crate) modules: Vec<Module>,
    pub(crate) headers: Vec<HrlFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) schemas: Vec<SchemaFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) partials: Vec<PartialFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(bound = "")]
pub struct Snapshot<S: state::SnapshotState = state::Unsorted> {
    #[serde(flatten)]
    parts: SnapshotParts,
    #[serde(skip)]
    _state: PhantomData<S>,
}

impl TryFrom<SnapshotParts> for Snapshot<state::Canonical> {
    type Error = SnapshotError;

    fn try_from(parts: SnapshotParts) -> Result<Self, SnapshotError> {
        if let Some(detail) = out_of_order(&parts) {
            return Err(SnapshotError::NotCanonicalValue { detail });
        }
        if parts.header.format_version >= 5
            && let Some(detail) = missing_app(&parts)
        {
            return Err(SnapshotError::NotCanonicalValue { detail });
        }
        Ok(Self::from_canonical_parts(parts))
    }
}

impl<'de> Deserialize<'de> for Snapshot<state::Canonical> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let parts = SnapshotParts::deserialize(deserializer)?;
        Snapshot::try_from(parts).map_err(D::Error::custom)
    }
}

impl Snapshot<state::Unsorted> {
    pub fn from_extracted(
        header: SnapshotHeader,
        modules: Vec<Module>,
        headers: Vec<HrlFile>,
    ) -> Self {
        Self {
            parts: SnapshotParts {
                header,
                apps: Vec::new(),
                modules,
                headers,
                schemas: Vec::new(),
                partials: Vec::new(),
            },
            _state: PhantomData,
        }
    }

    #[must_use]
    pub fn with_apps(mut self, apps: Vec<AppManifest>) -> Self {
        self.parts.apps = apps;
        self
    }

    #[must_use]
    pub fn with_cuttlefish(mut self, schemas: Vec<SchemaFile>, partials: Vec<PartialFile>) -> Self {
        self.parts.schemas = schemas;
        self.parts.partials = partials;
        self
    }

    #[must_use]
    pub fn into_canonical(mut self) -> Snapshot<state::Canonical> {
        canonicalize_parts(&mut self.parts);
        Snapshot {
            parts: self.parts,
            _state: PhantomData,
        }
    }
}

impl<S: state::SnapshotState> Snapshot<S> {
    pub fn header(&self) -> &SnapshotHeader {
        &self.parts.header
    }

    pub fn modules(&self) -> &[Module] {
        &self.parts.modules
    }

    pub fn headers(&self) -> &[HrlFile] {
        &self.parts.headers
    }

    pub fn apps(&self) -> &[AppManifest] {
        &self.parts.apps
    }

    pub fn schemas(&self) -> &[SchemaFile] {
        &self.parts.schemas
    }

    pub fn partials(&self) -> &[PartialFile] {
        &self.parts.partials
    }

    /// Records are declared in headers (.hrl) and inline in modules (.erl): both.
    pub fn records(&self) -> impl Iterator<Item = &RecordDecl> {
        self.parts
            .headers
            .iter()
            .flat_map(|h| h.records.iter())
            .chain(self.parts.modules.iter().flat_map(|m| m.records.iter()))
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
        self.module_named(module)
            .is_some_and(|m| m.exports_function(function, arity))
    }

    /// Look up a module by name in O(log n): canonical snapshots keep
    /// `modules` sorted by `name`.
    pub fn module_named(&self, name: &ModuleName) -> Option<&Module> {
        self.parts
            .modules
            .binary_search_by(|m| m.name.cmp(name))
            .ok()
            .map(|idx| &self.parts.modules[idx])
    }

    /// Every module whose `-behaviour(B).` list contains `behaviour`.
    /// Reverse index over `Module.behaviours`; built per call (one
    /// linear scan over `self.modules`). Used by behaviour-conformance
    /// checks and the R7 suite-implementer sweep.
    pub fn implementers_of(&self, behaviour: &ModuleName) -> Vec<&Module> {
        self.parts
            .modules
            .iter()
            .filter(|m| m.behaviours.contains(behaviour))
            .collect()
    }

    pub(crate) fn from_canonical_parts(parts: SnapshotParts) -> Self {
        Self {
            parts,
            _state: PhantomData,
        }
    }

    #[must_use]
    pub fn written_format_version(&self) -> u32 {
        self.parts
            .header
            .format_version
            .clamp(LAST_MIGRATABLE_FORMAT_VERSION, FORMAT_VERSION)
    }

    #[must_use]
    pub fn unwritable_reason(&self) -> Option<String> {
        if self.written_format_version() < 5 {
            return None;
        }
        missing_app(&self.parts)
    }
}

impl Default for Snapshot<state::Unsorted> {
    fn default() -> Self {
        Self::from_extracted(
            SnapshotHeader {
                project: ProjectName::new("placeholder").expect("valid"),
                tag: TagName::new("v0.0.0").expect("valid"),
                branch: None,
                commit: CommitSha::new("0".repeat(40)).expect("valid"),
                scanned_paths: Vec::new(),
                apps_scanned: Vec::new(),
                generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
                generated_at: OffsetDateTime::UNIX_EPOCH,
                extractor_version: String::new(),
                format_version: FORMAT_VERSION,
                dep_pins: Vec::new(),
                host_facts: None,
                extraction_settings: None,
            },
            Vec::new(),
            Vec::new(),
        )
    }
}
