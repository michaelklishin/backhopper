// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::LazyLock;

use backhopper_erlang_scan::{ScanArity, scan_arity};
use regex::Regex;

use crate::jigsaw::plugin_set::{PluginFile, PluginSet};
use crate::jigsaw::references::{
    NotChecked, Reference, ReferenceSet, ReferenceSite, SiteDetail, UncheckedSite, UncheckedSymbol,
};
use crate::jigsaw::surface::{AppPresence, Lookup, OtpAppPresence, OtpSurface, ReleaseSurface};
use crate::model::macro_decl::{MacroBody, MacroKey};
use crate::model::names::{
    ApplicationName, Arity, BehaviourName, ConfKey, FieldName, FunctionName, MacroName, Mfa,
    ModuleName, RecordName, SpecText, TypeName, ValidatorName,
};
use crate::model::non_empty::NonEmpty;
use crate::model::normalised::NormalisedDefault;
use crate::model::schema_files::PartialRef;
use crate::model::snapshot::{FunArity, HeaderRef, Module};

/// What the build patch says about one reference, with the places the
/// plugin set uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor<K> {
    pub kind: K,
    pub sites: BTreeSet<ReferenceSite>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallAnchor {
    pub mfa: Mfa,
    pub spec: Option<SpecText>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordAnchor {
    pub roots: BTreeSet<HeaderRef>,
    pub header: HeaderRef,
    pub name: RecordName,
    pub fields: Vec<FieldName>,
    pub defaults: BTreeMap<FieldName, Option<NormalisedDefault>>,
    pub unset: BTreeSet<FieldName>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacroAtBuild {
    pub key: MacroKey,
    pub header: HeaderRef,
    pub body: MacroBody,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacroAnchor {
    pub roots: BTreeSet<HeaderRef>,
    pub entries: Vec<MacroAtBuild>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BehaviourAnchor {
    pub implementer: ModuleName,
    pub behaviour: BehaviourName,
    pub module: ModuleName,
    pub implementer_exports: BTreeSet<FunArity>,
    pub build_required: BTreeSet<FunArity>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartialAnchor {
    pub partial: PartialRef,
    pub keys: BTreeSet<ConfKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnchor {
    pub module: ModuleName,
    pub name: TypeName,
    pub arity: Arity,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AppAnchor {
    pub app: ApplicationName,
    pub required_by: NonEmpty<ApplicationName>,
}

/// A reference the build patch satisfies, checked once per host release.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostAnchor {
    Call(CallAnchor),
    Header(HeaderRef),
    Record(RecordAnchor),
    Macro(MacroAnchor),
    Behaviour(BehaviourAnchor),
    Type(TypeAnchor),
    Partial(PartialAnchor),
    Validator(ValidatorName),
    App(AppAnchor),
}

/// A reference only OTP satisfies, checked once per OTP tag. OTP has no
/// build side, so these ask for presence only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum OtpAnchor {
    Call(Mfa),
    Type(TypeAnchor),
    Behaviour {
        name: BehaviourName,
        module: ModuleName,
    },
    App(AppAnchor),
}

/// An app the plugin set needs that the build patch does not ship either:
/// missing at every target.
pub type UnprovidedApp = AppAnchor;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchored {
    pub host: Vec<Anchor<HostAnchor>>,
    pub otp: Vec<Anchor<OtpAnchor>>,
    pub unprovided: Vec<Anchor<UnprovidedApp>>,
    pub not_checked: BTreeMap<NotChecked, usize>,
    pub unchecked: BTreeSet<UncheckedSite>,
}

impl Anchored {
    fn count(&mut self, reason: NotChecked) {
        *self.not_checked.entry(reason).or_default() += 1;
    }

    fn list_unresolved(&mut self, reference: &Reference, sites: &BTreeSet<ReferenceSite>) {
        self.unchecked
            .extend(sites.iter().map(|site| UncheckedSite {
                symbol: UncheckedSymbol::UnresolvedAtBuild(reference.clone()),
                file: site.file.clone(),
                line: site.line,
            }));
    }

    /// Distinct references compared at every target.
    #[must_use]
    pub fn checked(&self) -> usize {
        self.host.len() + self.otp.len() + self.unprovided.len()
    }
}

static MACRO_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\?\s*([A-Za-z_][A-Za-z0-9_]*)").expect("regex"));

const PREDEFINED_MACROS: &[&str] = &[
    "MODULE",
    "MODULE_STRING",
    "FILE",
    "LINE",
    "MACHINE",
    "FUNCTION_NAME",
    "FUNCTION_ARITY",
    "OTP_RELEASE",
    "BASE_MODULE",
    "BASE_MODULE_STRING",
];

#[must_use]
pub fn is_predefined_macro(name: &str) -> bool {
    PREDEFINED_MACROS.contains(&name)
}

#[must_use]
pub fn find_spec(module: &Module, function: &FunctionName, arity: Arity) -> Option<SpecText> {
    module
        .specs
        .iter()
        .find(|spec| &spec.name == function && spec.arity == arity)
        .and_then(|spec| SpecText::new(spec.signature.clone()).ok())
}

fn is_elixir(module: &ModuleName) -> bool {
    let name = module.as_str().trim_matches('\'');
    name.starts_with("Elixir.")
}

/// Splits what the build patch cannot satisfy into the three kinds the
/// report counts apart.
fn classify_unresolved(build: &ReleaseSurface<'_>, module: &ModuleName) -> NotChecked {
    if is_elixir(module) {
        NotChecked::Elixir
    } else if build.untracked_owner(module).is_some() {
        NotChecked::Untracked
    } else {
        NotChecked::UnresolvedAtBuild
    }
}

/// Splits `references` into what the build patch satisfies and what it
/// does not. An OTP export that the build patch lacks becomes an OTP
/// anchor. Every host-owned anchor also needs the app that owns it.
#[must_use]
pub fn anchor(
    plugins: &PluginSet,
    references: &ReferenceSet,
    build: &ReleaseSurface<'_>,
    otp: &[OtpSurface<'_>],
) -> Anchored {
    let mut out = Anchored::default();
    let mut found = Found::default();
    let mut needs: BTreeMap<ApplicationName, BTreeSet<ReferenceSite>> = BTreeMap::new();
    let mut anchoring = Anchoring {
        plugins,
        references,
        build,
        otp,
        out: &mut out,
        found: &mut found,
        needs: &mut needs,
    };
    for (reference, sites) in references.external() {
        anchoring.reference(reference, sites);
    }
    anchoring.apps();
    out.host = found.host.into_iter().map(Anchor::from).collect();
    out.otp = found.otp.into_iter().map(Anchor::from).collect();
    out
}

/// Anchors by what they compare, so two references that ask the same
/// question of the same targets (the same host macro used from two files
/// that include the same headers) are one anchor with both sites.
#[derive(Default)]
struct Found {
    host: BTreeMap<HostAnchor, BTreeSet<ReferenceSite>>,
    otp: BTreeMap<OtpAnchor, BTreeSet<ReferenceSite>>,
}

impl<K> From<(K, BTreeSet<ReferenceSite>)> for Anchor<K> {
    fn from((kind, sites): (K, BTreeSet<ReferenceSite>)) -> Self {
        Self { kind, sites }
    }
}

struct Anchoring<'a, 's> {
    plugins: &'a PluginSet,
    references: &'a ReferenceSet,
    build: &'a ReleaseSurface<'s>,
    otp: &'a [OtpSurface<'s>],
    out: &'a mut Anchored,
    found: &'a mut Found,
    needs: &'a mut BTreeMap<ApplicationName, BTreeSet<ReferenceSite>>,
}

impl Anchoring<'_, '_> {
    fn host(&mut self, kind: HostAnchor, sites: &BTreeSet<ReferenceSite>, owner: &ApplicationName) {
        self.need(owner, sites);
        self.found
            .host
            .entry(kind)
            .or_default()
            .extend(sites.iter().cloned());
    }

    fn otp(&mut self, kind: OtpAnchor, sites: &BTreeSet<ReferenceSite>) {
        self.found
            .otp
            .entry(kind)
            .or_default()
            .extend(sites.iter().cloned());
    }

    fn need(&mut self, app: &ApplicationName, sites: &BTreeSet<ReferenceSite>) {
        self.needs
            .entry(app.clone())
            .or_default()
            .extend(sites.iter().cloned());
    }

    fn otp_exports(&self, mfa: &Mfa) -> bool {
        self.otp.iter().any(|surface| {
            surface
                .module(&mfa.module)
                .is_some_and(|m| m.exports_function(&mfa.function, mfa.arity))
        })
    }

    fn otp_exports_type(&self, module: &ModuleName, name: &TypeName, arity: Arity) -> bool {
        self.otp.iter().any(|surface| {
            surface
                .module(module)
                .is_some_and(|m| m.exports_type(name, arity))
        })
    }

    fn reference(&mut self, reference: &Reference, sites: &BTreeSet<ReferenceSite>) {
        match reference {
            Reference::Call(mfa) => self.call(mfa, sites),
            Reference::Record { includer, name } => {
                let roots = self.roots_of(includer);
                self.record(&roots, includer, name, sites);
            }
            Reference::Macro { includer, key } => {
                let roots = self.roots_of(includer);
                self.macro_use(&roots, includer, key, sites);
            }
            Reference::Header(header) => self.header(header, sites),
            Reference::Behaviour {
                implementer,
                behaviour,
            } => self.behaviour(implementer, behaviour, sites),
            Reference::Type {
                module,
                name,
                arity,
            } => self.type_ref(module, name, *arity, sites),
            Reference::Partial { partial, keys } => self.partial(partial, keys, sites),
            Reference::Validator(name) => self.validator(name, sites),
            Reference::App(app) => self.need(app, sites),
        }
    }

    fn roots_of(&self, includer: &PluginFile) -> BTreeSet<HeaderRef> {
        self.references
            .host_includes(includer)
            .cloned()
            .unwrap_or_default()
    }

    fn call(&mut self, mfa: &Mfa, sites: &BTreeSet<ReferenceSite>) {
        if let Some(entry) = self.build.module(&mfa.module)
            && entry.module.exports_function(&mfa.function, mfa.arity)
        {
            let spec = find_spec(entry.module, &mfa.function, mfa.arity);
            let owner = entry.app.clone();
            self.host(
                HostAnchor::Call(CallAnchor {
                    mfa: mfa.clone(),
                    spec,
                }),
                sites,
                &owner,
            );
        } else if self.otp_exports(mfa) {
            self.otp(OtpAnchor::Call(mfa.clone()), sites);
        } else {
            self.unmatched(
                classify_unresolved(self.build, &mfa.module),
                &Reference::Call(mfa.clone()),
                sites,
            );
        }
    }

    fn unmatched(
        &mut self,
        reason: NotChecked,
        reference: &Reference,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        if reason == NotChecked::UnresolvedAtBuild {
            self.out.list_unresolved(reference, sites);
        } else {
            self.out.count(reason);
        }
    }

    fn header(&mut self, header: &HeaderRef, sites: &BTreeSet<ReferenceSite>) {
        if self.build.header(header).is_some() {
            self.host(HostAnchor::Header(header.clone()), sites, &header.app);
        } else if !self
            .otp
            .iter()
            .any(|surface| surface.app(&header.app) != OtpAppPresence::Absent)
        {
            self.out
                .list_unresolved(&Reference::Header(header.clone()), sites);
        }
    }

    /// A use found nowhere is the includer's problem to report when one of
    /// its includes could not be read; otherwise it is a count.
    fn not_found(
        &mut self,
        includer: &PluginFile,
        reference: &Reference,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        if !self.references.has_unresolved_include(includer) {
            self.out.list_unresolved(reference, sites);
        }
    }

    fn record(
        &mut self,
        roots: &BTreeSet<HeaderRef>,
        includer: &PluginFile,
        name: &RecordName,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        let Some((header, decl)) = self.build.find_record(roots, name) else {
            let reference = Reference::Record {
                includer: includer.clone(),
                name: name.clone(),
            };
            self.not_found(includer, &reference, sites);
            return;
        };
        let unset = sites
            .iter()
            .filter_map(|site| match &site.detail {
                SiteDetail::Construction(construction) => Some(&construction.unset),
                SiteDetail::Plain | SiteDetail::Call(_) => None,
            })
            .flatten()
            .cloned()
            .collect();
        let anchor = RecordAnchor {
            roots: roots.clone(),
            header: header.clone(),
            name: name.clone(),
            fields: decl.fields.iter().map(|f| f.name.clone()).collect(),
            defaults: decl
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.default.clone()))
                .collect(),
            unset,
        };
        self.host(HostAnchor::Record(anchor), sites, &header.app);
    }

    fn macro_use(
        &mut self,
        roots: &BTreeSet<HeaderRef>,
        includer: &PluginFile,
        key: &MacroKey,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        let mut entries = Vec::new();
        let mut visited = BTreeSet::new();
        self.collect_macro(roots, key, &mut visited, &mut entries);
        let Some(first) = entries.first() else {
            let reference = Reference::Macro {
                includer: includer.clone(),
                key: key.clone(),
            };
            self.not_found(includer, &reference, sites);
            return;
        };
        let owner = first.header.app.clone();
        let anchor = MacroAnchor {
            roots: roots.clone(),
            entries,
        };
        self.host(HostAnchor::Macro(anchor), sites, &owner);
    }

    /// The macro and, transitively, every macro its body names, as the
    /// build patch defines them: all of them are expanded into the plugin.
    fn collect_macro(
        &self,
        roots: &BTreeSet<HeaderRef>,
        key: &MacroKey,
        visited: &mut BTreeSet<MacroKey>,
        entries: &mut Vec<MacroAtBuild>,
    ) {
        let found = self.build.find_macro(roots, key).or_else(|| {
            key.arity.and_then(|_| {
                self.build
                    .find_macro(roots, &MacroKey::object_like(key.name.clone()))
            })
        });
        let Some((header, decl)) = found else {
            return;
        };
        if !visited.insert(decl.key.clone()) {
            return;
        }
        entries.push(MacroAtBuild {
            key: decl.key.clone(),
            header,
            body: decl.body.clone(),
        });
        let MacroBody::Text(body) = &decl.body else {
            return;
        };
        for used in macro_uses(body.as_str()) {
            self.collect_macro(roots, &used, visited, entries);
        }
    }

    fn behaviour(
        &mut self,
        implementer: &ModuleName,
        behaviour: &BehaviourName,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        let Ok(module) = ModuleName::from_str(behaviour.as_str()) else {
            return;
        };
        if let Some(entry) = self.build.module(&module) {
            let required = required_callbacks(entry.module);
            let anchor = BehaviourAnchor {
                implementer: implementer.clone(),
                behaviour: behaviour.clone(),
                module,
                implementer_exports: self
                    .references
                    .exports_of(implementer)
                    .cloned()
                    .unwrap_or_default(),
                build_required: required,
            };
            let owner = entry.app.clone();
            self.host(HostAnchor::Behaviour(anchor), sites, &owner);
        } else if self
            .otp
            .iter()
            .any(|surface| surface.module(&module).is_some())
        {
            self.otp(
                OtpAnchor::Behaviour {
                    name: behaviour.clone(),
                    module,
                },
                sites,
            );
        } else {
            self.unmatched(
                classify_unresolved(self.build, &module),
                &Reference::Behaviour {
                    implementer: implementer.clone(),
                    behaviour: behaviour.clone(),
                },
                sites,
            );
        }
    }

    fn type_ref(
        &mut self,
        module: &ModuleName,
        name: &TypeName,
        arity: Arity,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        let anchor = TypeAnchor {
            module: module.clone(),
            name: name.clone(),
            arity,
        };
        if let Some(entry) = self.build.module(module)
            && entry.module.exports_type(name, arity)
        {
            let owner = entry.app.clone();
            self.host(HostAnchor::Type(anchor), sites, &owner);
        } else if self.otp_exports_type(module, name, arity) {
            self.otp(OtpAnchor::Type(anchor), sites);
        } else {
            self.unmatched(
                classify_unresolved(self.build, module),
                &Reference::Type {
                    module: module.clone(),
                    name: name.clone(),
                    arity,
                },
                sites,
            );
        }
    }

    fn partial(
        &mut self,
        partial: &PartialRef,
        keys: &BTreeSet<ConfKey>,
        sites: &BTreeSet<ReferenceSite>,
    ) {
        match self.build.partial(partial) {
            Lookup::Found(file) => {
                let anchor = PartialAnchor {
                    partial: partial.clone(),
                    keys: keys.intersection(&file.mappings).cloned().collect(),
                };
                self.host(HostAnchor::Partial(anchor), sites, &partial.app);
            }
            Lookup::Missing | Lookup::Stopped(_) => {
                if self.build.untracked_dependency(&partial.app) {
                    self.out.count(NotChecked::Untracked);
                } else {
                    let reference = Reference::Partial {
                        partial: partial.clone(),
                        keys: keys.clone(),
                    };
                    self.out.list_unresolved(&reference, sites);
                }
            }
        }
    }

    fn validator(&mut self, name: &ValidatorName, sites: &BTreeSet<ReferenceSite>) {
        match self.build.validator(name) {
            Lookup::Found(app) => {
                let owner = app.clone();
                self.host(HostAnchor::Validator(name.clone()), sites, &owner);
            }
            Lookup::Missing | Lookup::Stopped(_) => {
                self.out
                    .list_unresolved(&Reference::Validator(name.clone()), sites);
            }
        }
    }

    /// Settles every app the plugin set needs: OTP's if some OTP tag in the
    /// run provides it, otherwise the host's, or missing everywhere when the
    /// build patch itself does not ship it.
    fn apps(&mut self) {
        let needs = std::mem::take(self.needs);
        for (app, sites) in needs {
            if self.plugins.contains_app(&app) {
                continue;
            }
            let required_by: BTreeSet<ApplicationName> =
                sites.iter().map(|site| site.file.app.clone()).collect();
            let Ok(required_by) = NonEmpty::try_from(required_by.into_iter().collect::<Vec<_>>())
            else {
                continue;
            };
            let anchor = AppAnchor { app, required_by };
            if self
                .otp
                .iter()
                .any(|surface| surface.app(&anchor.app) != OtpAppPresence::Absent)
            {
                self.otp(OtpAnchor::App(anchor), &sites);
            } else if self.build.shipped().lookup(&anchor.app) == AppPresence::Absent {
                self.out.unprovided.push(Anchor {
                    kind: anchor,
                    sites,
                });
            } else {
                self.found
                    .host
                    .entry(HostAnchor::App(anchor))
                    .or_default()
                    .extend(sites);
            }
        }
    }
}

/// The macros a body names with `?NAME`, with the arity of a use that has
/// an argument list.
fn macro_uses(body: &str) -> Vec<MacroKey> {
    let mut keys = Vec::new();
    for caps in MACRO_USE.captures_iter(body) {
        let whole = caps.get(0).expect("capture");
        let name = &caps[1];
        if is_predefined_macro(name) {
            continue;
        }
        let Ok(name) = MacroName::from_str(name) else {
            continue;
        };
        let rest = &body[whole.end()..];
        let arity = rest
            .trim_start()
            .strip_prefix('(')
            .and_then(|after| match scan_arity(after) {
                ScanArity::Exact(n) => Some(Arity::new(n)),
                ScanArity::Unterminated => None,
            });
        keys.push(MacroKey { name, arity });
    }
    keys
}

/// The callbacks a behaviour module requires: declared and not optional.
#[must_use]
pub fn required_callbacks(module: &Module) -> BTreeSet<FunArity> {
    let optional: BTreeSet<&FunArity> = module.optional_callbacks.iter().collect();
    module
        .callbacks
        .iter()
        .map(|callback| FunArity {
            name: callback.name.clone(),
            arity: callback.arity,
        })
        .filter(|callback| !optional.contains(callback))
        .collect()
}
