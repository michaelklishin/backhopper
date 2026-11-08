// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::jigsaw::plugin_set::{
    PluginAppSources, PluginFile, PluginSet, PluginSources, SourceFile,
};
use backhopper_core::jigsaw::references::{Reference, ReferenceSet, ReferenceSite};
use backhopper_core::jigsaw::surface::{PinApps, ReleaseSurface, SurfaceSource};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::macro_decl::MacroKey;
use backhopper_core::model::names::{
    ApplicationName, Arity, DependencyName, MacroName, Mfa, RelativePath,
};
use backhopper_core::model::snapshot::{Snapshot, state};

pub(crate) struct AppSpec<'a> {
    pub(crate) name: &'a str,
    pub(crate) requires: &'a [&'a str],
    pub(crate) files: &'a [(&'a str, &'a str)],
}

pub(crate) fn plugin_set(apps: &[AppSpec<'_>]) -> PluginSet {
    plugin_set_with_host_provided(apps, &[])
}

pub(crate) fn plugin_set_with_host_provided(apps: &[AppSpec<'_>], provided: &[&str]) -> PluginSet {
    let apps = apps
        .iter()
        .map(|spec| {
            let name = ApplicationName::new(spec.name).unwrap();
            let manifest = AppManifest {
                name: name.clone(),
                source: ManifestSource::AppSrc,
                requires: ManifestDeps::Listed(
                    spec.requires
                        .iter()
                        .map(|app| ApplicationName::new(*app).unwrap())
                        .collect(),
                ),
            }
            .into_read()
            .unwrap();
            PluginAppSources {
                root: PathBuf::from(format!("/plugins/{}", spec.name)),
                manifest,
                manifest_path: RelativePath::new(format!("src/{}.app.src", spec.name)).unwrap(),
                files: spec
                    .files
                    .iter()
                    .map(|(path, text)| SourceFile {
                        path: RelativePath::new(*path).unwrap(),
                        bytes: text.as_bytes().to_vec(),
                    })
                    .collect(),
            }
        })
        .collect();
    PluginSet::new(PluginSources {
        apps,
        host_provided: provided
            .iter()
            .map(|app| ApplicationName::new(*app).unwrap())
            .collect(),
    })
    .unwrap()
}

/// The build patch: one host snapshot, kept with what the surface borrows.
pub(crate) struct Build {
    pub(crate) snapshot: Snapshot<state::Canonical>,
    pub(crate) apps: PinApps,
    pub(crate) roots: Vec<ApplicationName>,
    pub(crate) untracked: BTreeSet<DependencyName>,
}

impl Build {
    pub(crate) fn new(snapshot: Snapshot<state::Canonical>, roots: &[&str]) -> Self {
        let apps = PinApps::of(&snapshot).unwrap();
        Self {
            snapshot,
            apps,
            roots: roots
                .iter()
                .map(|app| ApplicationName::new(*app).unwrap())
                .collect(),
            untracked: BTreeSet::new(),
        }
    }

    pub(crate) fn surface(&self) -> ReleaseSurface<'_> {
        ReleaseSurface::new(
            &[SurfaceSource {
                snapshot: &self.snapshot,
                apps: &self.apps,
            }],
            &self.roots,
            &self.untracked,
        )
    }
}

pub(crate) fn mfa(text: &str) -> Mfa {
    text.parse().unwrap()
}

pub(crate) fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

pub(crate) fn file(app_name: &str, path: &str) -> PluginFile {
    PluginFile {
        app: app(app_name),
        path: RelativePath::new(path).unwrap(),
    }
}

pub(crate) fn macro_key(name: &str, arity: Option<u8>) -> MacroKey {
    MacroKey {
        name: MacroName::new(name).unwrap(),
        arity: arity.map(Arity::new),
    }
}

/// The lines a reference is used at, in ascending order.
pub(crate) fn lines(references: &ReferenceSet, reference: &Reference) -> Vec<u32> {
    sites(references, reference)
        .iter()
        .map(|s| s.line)
        .collect()
}

pub(crate) fn sites<'a>(
    references: &'a ReferenceSet,
    reference: &Reference,
) -> Vec<&'a ReferenceSite> {
    references
        .external()
        .get(reference)
        .map(|sites| sites.iter().collect())
        .unwrap_or_default()
}
