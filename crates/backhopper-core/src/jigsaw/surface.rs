// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::iter;

use crate::jigsaw::findings::Stop;
use crate::jigsaw::plan::JigsawPlan;
use crate::model::app_manifest::{ManifestDeps, ManifestUnreadable};
use crate::model::macro_decl::{MacroDecl, MacroKey};
use crate::model::names::{ApplicationName, DependencyName, ModuleName, RecordName, ValidatorName};
use crate::model::pin::Pin;
use crate::model::schema_files::{PartialFile, PartialRef};
use crate::model::snapshot::{HeaderRef, HrlFile, Module, RecordDecl, Snapshot, state};

/// The app of every module and header of one snapshot, in snapshot order.
/// `load` makes one per snapshot, and only for a snapshot where no entry
/// lacks an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinApps {
    modules: Vec<ApplicationName>,
    headers: Vec<ApplicationName>,
}

impl PinApps {
    #[must_use]
    pub fn of(snapshot: &Snapshot<state::Canonical>) -> Option<Self> {
        let modules = snapshot
            .modules()
            .iter()
            .map(|m| m.app.clone())
            .collect::<Option<Vec<_>>>()?;
        let headers = snapshot
            .headers()
            .iter()
            .map(|h| h.app.clone())
            .collect::<Option<Vec<_>>>()?;
        Some(Self { modules, headers })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceSource<'s> {
    pub snapshot: &'s Snapshot<state::Canonical>,
    pub apps: &'s PinApps,
}

/// Whether an app is among the apps a side provides. Closures stop at
/// apps they cannot read through, which makes "not in the closure" an
/// answer of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPresence {
    Provided,
    Absent,
    Stopped(Stop),
}

/// An app the snapshot leaves out by configuration ships with OTP all the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtpAppPresence {
    Provided,
    Absent,
    Excluded,
}

/// Why the closure over manifests could not continue through an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    Manifest(ManifestUnreadable),
    Untracked,
}

impl Blocker {
    #[must_use]
    pub fn stop(&self) -> Stop {
        match self {
            Self::Manifest(_) => Stop::UnreadableManifest,
            Self::Untracked => Stop::UntrackedApp,
        }
    }
}

/// The apps a host release ships: the closure of its release roots over the
/// manifests of the host and its pinned dependencies. There is no
/// `contains`, so a closure that stopped short cannot be read as a `bool`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippedApps {
    provided: BTreeSet<ApplicationName>,
    blockers: BTreeMap<ApplicationName, Blocker>,
}

impl ShippedApps {
    fn closure(
        roots: &[ApplicationName],
        manifests: &BTreeMap<&ApplicationName, &ManifestDeps>,
        untracked: &BTreeSet<DependencyName>,
    ) -> Self {
        let mut provided = BTreeSet::new();
        let mut blockers = BTreeMap::new();
        let mut queue: VecDeque<&ApplicationName> = roots.iter().collect();
        while let Some(app) = queue.pop_front() {
            if !provided.insert(app.clone()) {
                continue;
            }
            match manifests.get(app) {
                Some(ManifestDeps::Listed(requires)) => queue.extend(requires),
                Some(ManifestDeps::Unreadable(reason)) => {
                    blockers.insert(app.clone(), Blocker::Manifest(reason.clone()));
                }
                None if untracked.iter().any(|dep| dep.as_str() == app.as_str()) => {
                    blockers.insert(app.clone(), Blocker::Untracked);
                }
                None => {}
            }
        }
        Self { provided, blockers }
    }

    #[must_use]
    pub fn lookup(&self, app: &ApplicationName) -> AppPresence {
        if self.provided.contains(app) {
            AppPresence::Provided
        } else {
            self.stopped()
                .map_or(AppPresence::Absent, AppPresence::Stopped)
        }
    }

    #[must_use]
    pub fn stopped(&self) -> Option<Stop> {
        self.blockers.values().map(Blocker::stop).min()
    }

    /// The apps the closure stopped at, with why.
    #[must_use]
    pub fn blockers(&self) -> &BTreeMap<ApplicationName, Blocker> {
        &self.blockers
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModuleEntry<'s> {
    pub module: &'s Module,
    pub app: &'s ApplicationName,
}

/// What a schema-element lookup found, or why it could not tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup<T> {
    Found(T),
    Missing,
    Stopped(Stop),
}

/// One host release as the analysis reads it: the host snapshot and its
/// pinned dependency snapshots, indexed.
#[derive(Debug)]
pub struct ReleaseSurface<'s> {
    modules: BTreeMap<&'s ModuleName, ModuleEntry<'s>>,
    headers: BTreeMap<HeaderRef, &'s HrlFile>,
    partials: BTreeMap<PartialRef, &'s PartialFile>,
    validators: BTreeMap<&'s ValidatorName, Vec<&'s ApplicationName>>,
    apps: BTreeSet<&'s ApplicationName>,
    shipped: ShippedApps,
    untracked: &'s BTreeSet<DependencyName>,
}

impl<'s> ReleaseSurface<'s> {
    #[must_use]
    pub fn new(
        sources: &[SurfaceSource<'s>],
        roots: &[ApplicationName],
        untracked: &'s BTreeSet<DependencyName>,
    ) -> Self {
        let mut modules = BTreeMap::new();
        let mut headers = BTreeMap::new();
        let mut partials = BTreeMap::new();
        let mut validators: BTreeMap<&ValidatorName, Vec<&ApplicationName>> = BTreeMap::new();
        let mut manifests: BTreeMap<&ApplicationName, &ManifestDeps> = BTreeMap::new();
        for source in sources {
            let snapshot = source.snapshot;
            for (module, app) in snapshot.modules().iter().zip(&source.apps.modules) {
                modules
                    .entry(&module.name)
                    .or_insert(ModuleEntry { module, app });
            }
            for (header, app) in snapshot.headers().iter().zip(&source.apps.headers) {
                let reference = HeaderRef {
                    app: app.clone(),
                    app_path: header.app_path().unwrap_or_else(|| header.path.clone()),
                };
                headers.entry(reference).or_insert(header);
            }
            for partial in snapshot.partials() {
                partials.entry(partial.partial_ref()).or_insert(partial);
                for name in &partial.validators {
                    validators.entry(name).or_default().push(&partial.app);
                }
            }
            for schema in snapshot.schemas() {
                for name in &schema.validators {
                    validators.entry(name).or_default().push(&schema.app);
                }
            }
            for manifest in snapshot.apps() {
                manifests
                    .entry(&manifest.name)
                    .or_insert(&manifest.requires);
            }
        }
        let apps = manifests.keys().copied().collect();
        Self {
            modules,
            headers,
            partials,
            validators,
            apps,
            shipped: ShippedApps::closure(roots, &manifests, untracked),
            untracked,
        }
    }

    #[must_use]
    pub fn module(&self, name: &ModuleName) -> Option<&ModuleEntry<'s>> {
        self.modules.get(name)
    }

    #[must_use]
    pub fn header(&self, reference: &HeaderRef) -> Option<&'s HrlFile> {
        self.headers.get(reference).copied()
    }

    #[must_use]
    pub fn shipped(&self) -> &ShippedApps {
        &self.shipped
    }

    /// The headers reachable from `roots` through their `-include` lines,
    /// roots first. A header the release lacks ends its branch.
    #[must_use]
    pub fn header_closure(&self, roots: &BTreeSet<HeaderRef>) -> Vec<(HeaderRef, &'s HrlFile)> {
        let mut seen: BTreeSet<HeaderRef> = BTreeSet::new();
        let mut queue: VecDeque<HeaderRef> = roots.iter().cloned().collect();
        let mut out = Vec::new();
        while let Some(reference) = queue.pop_front() {
            if !seen.insert(reference.clone()) {
                continue;
            }
            let Some(header) = self.header(&reference) else {
                continue;
            };
            queue.extend(header.includes.iter().cloned());
            out.push((reference, header));
        }
        out
    }

    #[must_use]
    pub fn find_record(
        &self,
        roots: &BTreeSet<HeaderRef>,
        name: &RecordName,
    ) -> Option<(HeaderRef, &'s RecordDecl)> {
        self.header_closure(roots)
            .into_iter()
            .find_map(|(reference, header)| {
                header
                    .records
                    .iter()
                    .find(|record| &record.name == name)
                    .map(|record| (reference, record))
            })
    }

    #[must_use]
    pub fn find_macro(
        &self,
        roots: &BTreeSet<HeaderRef>,
        key: &MacroKey,
    ) -> Option<(HeaderRef, &'s MacroDecl)> {
        self.header_closure(roots)
            .into_iter()
            .find_map(|(reference, header)| {
                header
                    .defines
                    .iter()
                    .find(|decl| &decl.key == key)
                    .map(|decl| (reference, decl))
            })
    }

    /// Whether any root header exists in the release.
    #[must_use]
    pub fn has_any_header(&self, roots: &BTreeSet<HeaderRef>) -> bool {
        roots.iter().any(|root| self.headers.contains_key(root))
    }

    #[must_use]
    pub fn untracked_dependency(&self, app: &ApplicationName) -> bool {
        self.untracked
            .iter()
            .any(|dep| dep.as_str() == app.as_str())
    }

    /// Every app the release's snapshots carry a manifest for, shipped or
    /// not.
    #[must_use]
    pub fn tree_apps(&self) -> &BTreeSet<&'s ApplicationName> {
        &self.apps
    }

    /// Cuttlefish loads only the schemas of the apps on the node, so a
    /// partial is found only in an app the release ships.
    #[must_use]
    pub fn partial(&self, reference: &PartialRef) -> Lookup<&'s PartialFile> {
        match self.shipped.lookup(&reference.app) {
            AppPresence::Provided => self
                .partials
                .get(reference)
                .map_or(Lookup::Missing, |file| Lookup::Found(*file)),
            AppPresence::Absent => Lookup::Missing,
            AppPresence::Stopped(stop) => Lookup::Stopped(stop),
        }
    }

    /// The shipped app that defines a validator. A closure that stopped
    /// short may have missed the app that does.
    #[must_use]
    pub fn validator(&self, name: &ValidatorName) -> Lookup<&'s ApplicationName> {
        let owners = self.validators.get(name).map_or(&[][..], Vec::as_slice);
        if let Some(app) = owners
            .iter()
            .find(|app| self.shipped.lookup(app) == AppPresence::Provided)
        {
            return Lookup::Found(app);
        }
        self.shipped
            .stopped()
            .map_or(Lookup::Missing, Lookup::Stopped)
    }

    /// A module the host's snapshots lack, owned by a dependency the plan
    /// does not track: the module is the dependency's name, or starts with
    /// it and an underscore.
    #[must_use]
    pub fn untracked_owner(&self, module: &ModuleName) -> Option<&'s DependencyName> {
        let name = module.as_str();
        self.untracked.iter().find(|dep| {
            name == dep.as_str()
                || name
                    .strip_prefix(dep.as_str())
                    .is_some_and(|rest| rest.starts_with('_'))
        })
    }
}

/// One OTP tag: modules, exports and apps only.
#[derive(Debug)]
pub struct OtpSurface<'s> {
    snapshot: &'s Snapshot<state::Canonical>,
    apps: BTreeSet<&'s ApplicationName>,
    excluded: &'s BTreeSet<ApplicationName>,
}

impl<'s> OtpSurface<'s> {
    #[must_use]
    pub fn new(
        snapshot: &'s Snapshot<state::Canonical>,
        excluded: &'s BTreeSet<ApplicationName>,
    ) -> Self {
        Self {
            snapshot,
            apps: snapshot.apps().iter().map(|app| &app.name).collect(),
            excluded,
        }
    }

    #[must_use]
    pub fn module(&self, name: &ModuleName) -> Option<&'s Module> {
        self.snapshot.module_named(name)
    }

    /// An OTP app the snapshot leaves out by configuration ships with OTP
    /// all the same, so it is not absent.
    #[must_use]
    pub fn app(&self, app: &ApplicationName) -> OtpAppPresence {
        if self.apps.contains(app) {
            OtpAppPresence::Provided
        } else if self.excluded.contains(app) {
            OtpAppPresence::Excluded
        } else {
            OtpAppPresence::Absent
        }
    }
}

/// Every release and OTP tag of a loaded plan, in plan order.
#[derive(Debug)]
pub struct PlanSurfaces<'s> {
    releases: Vec<ReleaseSurface<'s>>,
    otp: Vec<OtpSurface<'s>>,
}

impl<'s> PlanSurfaces<'s> {
    pub(crate) fn new(
        plan: &'s JigsawPlan,
        snapshots: &'s BTreeMap<Pin, Snapshot<state::Canonical>>,
        pin_apps: &'s BTreeMap<Pin, PinApps>,
    ) -> Self {
        let source = |pin: &Pin| {
            Some(SurfaceSource {
                snapshot: snapshots.get(pin)?,
                apps: pin_apps.get(pin)?,
            })
        };
        let releases = plan
            .releases()
            .iter()
            .map(|release| {
                let host_pin = Pin::new(plan.host_project().clone(), release.tag.tag().clone());
                let roots: &[ApplicationName] = snapshots
                    .get(&host_pin)
                    .and_then(|snapshot| snapshot.header().host_facts.as_ref())
                    .map_or(&[], |facts| facts.release_roots.as_slice());
                let pins = release
                    .pins
                    .iter()
                    .map(|(project, tag)| Pin::new(project.clone(), tag.clone()));
                let sources: Vec<_> = iter::once(host_pin)
                    .chain(pins)
                    .filter_map(|pin| source(&pin))
                    .collect();
                ReleaseSurface::new(&sources, roots, plan.untracked_dependencies())
            })
            .collect();
        let otp = plan
            .otp_lines()
            .iter()
            .flat_map(|line| line.tags().iter())
            .filter_map(|tag| {
                let pin = Pin::new(plan.otp_project().clone(), tag.tag().clone());
                snapshots
                    .get(&pin)
                    .map(|snapshot| OtpSurface::new(snapshot, plan.excluded_otp_apps()))
            })
            .collect();
        Self { releases, otp }
    }

    /// The build patch: the newest release of the series.
    #[must_use]
    pub fn build(&self) -> &ReleaseSurface<'s> {
        &self.releases[self.releases.len() - 1]
    }

    #[must_use]
    pub fn releases(&self) -> &[ReleaseSurface<'s>] {
        &self.releases
    }

    #[must_use]
    pub fn otp(&self) -> &[OtpSurface<'s>] {
        &self.otp
    }
}
