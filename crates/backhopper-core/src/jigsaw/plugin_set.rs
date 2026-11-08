// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::LazyLock;

use regex::Regex;
use thiserror::Error;

use serde::{Deserialize, Serialize};

use crate::model::app_manifest::ReadManifest;
use crate::model::names::{ApplicationName, ModuleName, RelativePath, vocabulary};

vocabulary!(
    /// An app another app in the plugin set requires is bundled; the rest
    /// are plugins. The split is for the report: the analysis treats both
    /// the same.
    #[derive(PartialOrd, Ord, Hash)]
    pub enum AppRole: "app role" {
        Plugin => "plugin",
        Bundled => "bundled",
    }
);

/// A file of an app, addressed by the app and the path from the app root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginFile {
    pub app: ApplicationName,
    pub path: RelativePath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: RelativePath,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PluginAppSources {
    pub root: PathBuf,
    pub manifest: ReadManifest,
    pub manifest_path: RelativePath,
    pub files: Vec<SourceFile>,
}

/// What the directory walk found, as plain data.
#[derive(Debug, Clone, Default)]
pub struct PluginSources {
    pub apps: Vec<PluginAppSources>,
    pub host_provided: BTreeSet<ApplicationName>,
}

#[derive(Debug, Clone)]
pub struct PluginApp {
    root: PathBuf,
    manifest: ReadManifest,
    manifest_path: RelativePath,
    files: Vec<SourceFile>,
    role: AppRole,
}

impl PluginApp {
    #[must_use]
    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    #[must_use]
    pub fn manifest(&self) -> &ReadManifest {
        &self.manifest
    }

    #[must_use]
    pub fn manifest_path(&self) -> &RelativePath {
        &self.manifest_path
    }

    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    #[must_use]
    pub fn role(&self) -> AppRole {
        self.role
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PluginSetError {
    #[error("app {app} is found twice: {} and {}", first.display(), second.display())]
    DuplicateApp {
        app: ApplicationName,
        first: PathBuf,
        second: PathBuf,
    },
    #[error("module {module} is defined twice: {}/{} and {}/{}", first.app, first.path, second.app, second.path)]
    DuplicateModule {
        module: ModuleName,
        first: PluginFile,
        second: PluginFile,
    },
}

/// The plugins and every library they bundle, checked as one unit.
#[derive(Debug, Clone)]
pub struct PluginSet {
    apps: BTreeMap<ApplicationName, PluginApp>,
    host_provided: BTreeSet<ApplicationName>,
}

static MODULE_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*-\s*module\s*\(\s*([A-Za-z0-9_@.']+)\s*\)").expect("regex")
});

/// The module an `.erl` file declares with `-module(...)`.
#[must_use]
pub fn declared_module(file: &SourceFile) -> Option<ModuleName> {
    if !file.path.as_str().ends_with(".erl") {
        return None;
    }
    let text = str::from_utf8(&file.bytes).ok()?;
    let name = MODULE_ATTRIBUTE.captures(text)?.get(1)?.as_str();
    ModuleName::from_str(name).ok()
}

impl PluginSet {
    pub fn new(sources: PluginSources) -> Result<Self, PluginSetError> {
        let mut apps: BTreeMap<ApplicationName, PluginApp> = BTreeMap::new();
        let mut modules: BTreeMap<ModuleName, PluginFile> = BTreeMap::new();
        for app in sources.apps {
            let name = app.manifest.name().clone();
            if let Some(first) = apps.get(&name) {
                return Err(PluginSetError::DuplicateApp {
                    app: name,
                    first: first.root.clone(),
                    second: app.root,
                });
            }
            for file in &app.files {
                let Some(module) = declared_module(file) else {
                    continue;
                };
                let here = PluginFile {
                    app: name.clone(),
                    path: file.path.clone(),
                };
                if let Some(first) = modules.get(&module) {
                    return Err(PluginSetError::DuplicateModule {
                        module,
                        first: first.clone(),
                        second: here,
                    });
                }
                modules.insert(module, here);
            }
            apps.insert(
                name,
                PluginApp {
                    root: app.root,
                    manifest: app.manifest,
                    manifest_path: app.manifest_path,
                    files: app.files,
                    role: AppRole::Plugin,
                },
            );
        }
        let required: BTreeSet<ApplicationName> = apps
            .values()
            .flat_map(|app| app.manifest.requires().iter().cloned())
            .collect();
        for (name, app) in &mut apps {
            if required.contains(name) {
                app.role = AppRole::Bundled;
            }
        }
        Ok(Self {
            apps,
            host_provided: sources.host_provided,
        })
    }

    #[must_use]
    pub fn apps(&self) -> &BTreeMap<ApplicationName, PluginApp> {
        &self.apps
    }

    #[must_use]
    pub fn app(&self, name: &ApplicationName) -> Option<&PluginApp> {
        self.apps.get(name)
    }

    #[must_use]
    pub fn contains_app(&self, name: &ApplicationName) -> bool {
        self.apps.contains_key(name)
    }

    #[must_use]
    pub fn host_provided(&self) -> &BTreeSet<ApplicationName> {
        &self.host_provided
    }

    /// Every source file with the app it belongs to.
    pub fn files(&self) -> impl Iterator<Item = (PluginFile, &SourceFile)> {
        self.apps.iter().flat_map(|(app, plugin)| {
            plugin.files.iter().map(move |file| {
                (
                    PluginFile {
                        app: app.clone(),
                        path: file.path.clone(),
                    },
                    file,
                )
            })
        })
    }
}
