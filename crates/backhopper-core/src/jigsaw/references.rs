// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::jigsaw::plugin_set::PluginFile;
use crate::model::macro_decl::MacroKey;
use crate::model::names::{
    ApplicationName, Arity, BehaviourName, ConfKey, FieldName, FunctionName, Mfa, ModuleName,
    RecordName, TypeName, ValidatorName, vocabulary,
};
use crate::model::schema_files::PartialRef;
use crate::model::snapshot::{FunArity, HeaderRef};
use crate::model::verdict::IndirectCallForm;

vocabulary!(
    /// What a run did not compare, counted instead of reported as a finding.
    #[derive(PartialOrd, Ord, Hash)]
    pub enum NotChecked: "not-checked reason" {
        Internal => "internal",
        Dynamic => "dynamic",
        UnresolvedAtBuild => "unresolved_at_build",
        Elixir => "elixir",
        Untracked => "untracked",
        UnresolvedInclude => "unresolved_include",
        ConditionalSite => "conditional_site",
    }
);

/// How a call site spells the call. It is on the site, so one function
/// called two ways is one reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CallForm {
    Direct,
    FunRef,
    Apply,
    Indirect(IndirectCallForm),
}

/// The fields a `#name{...}` construction leaves to the record's default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordConstruction {
    pub unset: BTreeSet<FieldName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SiteDetail {
    Plain,
    Call(CallForm),
    Construction(RecordConstruction),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReferenceSite {
    pub file: PluginFile,
    pub line: u32,
    pub detail: SiteDetail,
}

/// Something the plugin set uses that it does not define itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reference {
    Call(Mfa),
    Record {
        includer: PluginFile,
        name: RecordName,
    },
    Macro {
        includer: PluginFile,
        key: MacroKey,
    },
    Header(HeaderRef),
    Behaviour {
        implementer: ModuleName,
        behaviour: BehaviourName,
    },
    Type {
        module: ModuleName,
        name: TypeName,
        arity: Arity,
    },
    Partial {
        partial: PartialRef,
        keys: BTreeSet<ConfKey>,
    },
    Validator(ValidatorName),
    App(ApplicationName),
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call(mfa) => write!(f, "{mfa}"),
            Self::Record { name, .. } => write!(f, "#{name}{{}}"),
            Self::Macro { key, .. } => write!(f, "?{key}"),
            Self::Header(header) => write!(f, "{header}"),
            Self::Behaviour { behaviour, .. } => write!(f, "{behaviour}"),
            Self::Type {
                module,
                name,
                arity,
            } => write!(f, "{module}:{name}/{arity}"),
            Self::Partial { partial, .. } => write!(f, "{partial}"),
            Self::Validator(name) => write!(f, "{name}"),
            Self::App(app) => write!(f, "{app}"),
        }
    }
}

/// A call whose module, function or arity the source does not spell out.
/// A part that is not known prints as `_`, and an unknown arity as `?`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DynamicCall {
    pub module: Option<ModuleName>,
    pub function: Option<FunctionName>,
    pub arity: Option<Arity>,
}

impl fmt::Display for DynamicCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.module {
            Some(module) => write!(f, "{module}")?,
            None => f.write_str("_")?,
        }
        f.write_str(":")?;
        match &self.function {
            Some(function) => write!(f, "{function}")?,
            None => f.write_str("_")?,
        }
        f.write_str("/")?;
        match self.arity {
            Some(arity) => write!(f, "{arity}"),
            None => f.write_str("?"),
        }
    }
}

/// What an unchecked site is about. The variant decides the reason, so a
/// site cannot be listed under a reason that does not fit it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UncheckedSymbol {
    UnresolvedAtBuild(Reference),
    Dynamic(DynamicCall),
}

impl UncheckedSymbol {
    #[must_use]
    pub fn reason(&self) -> NotChecked {
        match self {
            Self::UnresolvedAtBuild(_) => NotChecked::UnresolvedAtBuild,
            Self::Dynamic(_) => NotChecked::Dynamic,
        }
    }
}

impl fmt::Display for UncheckedSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnresolvedAtBuild(reference) => write!(f, "{reference}"),
            Self::Dynamic(call) => write!(f, "{call}"),
        }
    }
}

/// A place in the plugin set that a run did not compare, and what is
/// there. Sites order by reason, file, line and symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UncheckedSite {
    pub symbol: UncheckedSymbol,
    pub file: PluginFile,
    pub line: u32,
}

impl UncheckedSite {
    #[must_use]
    pub fn reason(&self) -> NotChecked {
        self.symbol.reason()
    }
}

impl Ord for UncheckedSite {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.reason(), &self.file, self.line, &self.symbol).cmp(&(
            other.reason(),
            &other.file,
            other.line,
            &other.symbol,
        ))
    }
}

impl PartialOrd for UncheckedSite {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Every external reference of a plugin set with the places it is used,
/// plus what collection already set aside as not checked.
#[derive(Debug, Clone, Default)]
pub struct ReferenceSet {
    external: BTreeMap<Reference, BTreeSet<ReferenceSite>>,
    host_includes: BTreeMap<PluginFile, BTreeSet<HeaderRef>>,
    exports: BTreeMap<ModuleName, BTreeSet<FunArity>>,
    unresolved_includes: BTreeSet<PluginFile>,
    not_checked: BTreeMap<NotChecked, usize>,
    unchecked: BTreeSet<UncheckedSite>,
}

impl ReferenceSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, reference: Reference, site: ReferenceSite) {
        self.external.entry(reference).or_default().insert(site);
    }

    pub fn count(&mut self, reason: NotChecked, n: usize) {
        if n > 0 {
            *self.not_checked.entry(reason).or_default() += n;
        }
    }

    pub fn add_unchecked(&mut self, site: UncheckedSite) {
        self.unchecked.insert(site);
    }

    pub fn set_host_includes(&mut self, file: PluginFile, headers: BTreeSet<HeaderRef>) {
        if !headers.is_empty() {
            self.host_includes.insert(file, headers);
        }
    }

    pub fn set_exports(&mut self, module: ModuleName, exports: BTreeSet<FunArity>) {
        self.exports.insert(module, exports);
    }

    pub fn mark_unresolved_include(&mut self, file: PluginFile) {
        self.unresolved_includes.insert(file);
    }

    #[must_use]
    pub fn external(&self) -> &BTreeMap<Reference, BTreeSet<ReferenceSite>> {
        &self.external
    }

    #[must_use]
    pub fn not_checked(&self) -> &BTreeMap<NotChecked, usize> {
        &self.not_checked
    }

    #[must_use]
    pub fn unchecked(&self) -> &BTreeSet<UncheckedSite> {
        &self.unchecked
    }

    /// The host headers a file includes, directly or through the plugin
    /// set's own headers.
    #[must_use]
    pub fn host_includes(&self, file: &PluginFile) -> Option<&BTreeSet<HeaderRef>> {
        self.host_includes.get(file)
    }

    #[must_use]
    pub fn exports_of(&self, module: &ModuleName) -> Option<&BTreeSet<FunArity>> {
        self.exports.get(module)
    }

    #[must_use]
    pub fn has_unresolved_include(&self, file: &PluginFile) -> bool {
        self.unresolved_includes.contains(file)
    }
}
