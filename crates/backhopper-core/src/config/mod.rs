// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper.toml` schema and loader.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::errors::{ConfigError, NameError};
use crate::model::names::{ProjectName, SeriesName, TagName};
use crate::model::pin::Pin;

pub const CONFIG_VERSION: u32 = 1;

const DEFAULT_SCAN_PATHS: &[&str] = &["src/**/*.erl", "src/**/*.ex", "include/**/*.hrl"];
const DEFAULT_SNAPSHOT_DIR: &str = "snapshots";
const DEFAULT_FALLBACK_BRANCH: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default = "default_config_version")]
    pub config_version: u32,

    #[serde(default)]
    pub defaults: DefaultsRaw,

    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectRaw>,

    #[serde(default, rename = "series")]
    pub series: Vec<SeriesRaw>,
}

fn default_config_version() -> u32 {
    CONFIG_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultsRaw {
    pub snapshot_dir: Option<String>,
    pub fallback_branch: Option<String>,
    pub scan_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRaw {
    pub name: String,
    pub git_url: String,
    pub language: Option<String>,
    pub tag_prefix: Option<String>,
    pub public_modules: Option<Vec<String>>,
    pub internal_modules: Option<Vec<String>>,
    pub scan_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesRaw {
    pub name: String,
    pub pins: Vec<PinRaw>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinRaw {
    pub project: String,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Defaults {
    pub snapshot_dir: PathBuf,
    pub fallback_branch: String,
    pub scan_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Erlang,
    Elixir,
}

impl FromStr for Language {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "erlang" => Ok(Self::Erlang),
            "elixir" => Ok(Self::Elixir),
            other => Err(ConfigError::Name(NameError::PatternMismatch {
                kind: "language",
                value: other.to_owned(),
                pattern: "erlang|elixir",
            })),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: ProjectName,
    pub git_url: PathBuf,
    pub language: Language,
    pub tag_prefix: String,
    pub public_modules: Vec<String>,
    pub internal_modules: Vec<String>,
    pub scan_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    pub name: SeriesName,
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Config {
    pub config_path: PathBuf,
    pub defaults: Defaults,
    pub projects: Vec<Project>,
    pub series: Vec<Series>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_path_buf()));
        }
        let text = fs::read_to_string(path)?;
        let raw: ConfigFile = toml::from_str(&text)?;
        Self::from_raw(path.to_path_buf(), raw)
    }

    pub fn from_raw(config_path: PathBuf, raw: ConfigFile) -> Result<Self, ConfigError> {
        if raw.config_version != CONFIG_VERSION {
            return Err(ConfigError::UnknownConfigVersion(raw.config_version));
        }
        let defaults = Defaults {
            snapshot_dir: raw
                .defaults
                .snapshot_dir
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SNAPSHOT_DIR)),
            fallback_branch: raw
                .defaults
                .fallback_branch
                .unwrap_or_else(|| DEFAULT_FALLBACK_BRANCH.to_owned()),
            scan_paths: raw
                .defaults
                .scan_paths
                .unwrap_or_else(|| DEFAULT_SCAN_PATHS.iter().map(|s| (*s).to_owned()).collect()),
        };
        let mut projects = Vec::with_capacity(raw.projects.len());
        for p in raw.projects {
            let language = p
                .language
                .as_deref()
                .map(Language::from_str)
                .transpose()?
                .unwrap_or(Language::Erlang);
            let scan_paths = p.scan_paths.unwrap_or_else(|| defaults.scan_paths.clone());
            projects.push(Project {
                name: ProjectName::new(p.name).map_err(ConfigError::Name)?,
                git_url: PathBuf::from(p.git_url),
                language,
                tag_prefix: p.tag_prefix.unwrap_or_else(|| String::from("v")),
                public_modules: p.public_modules.unwrap_or_default(),
                internal_modules: p.internal_modules.unwrap_or_default(),
                scan_paths,
            });
        }
        let mut series = Vec::with_capacity(raw.series.len());
        for s in raw.series {
            let mut pins = Vec::with_capacity(s.pins.len());
            for pin in s.pins {
                let project = ProjectName::new(pin.project.clone()).map_err(ConfigError::Name)?;
                if !projects.iter().any(|p| p.name == project) {
                    return Err(ConfigError::SeriesPinsUnknownProject {
                        series: s.name.clone(),
                        project: pin.project,
                    });
                }
                let tag = TagName::new(pin.tag).map_err(ConfigError::Name)?;
                pins.push(Pin::new(project, tag));
            }
            series.push(Series {
                name: SeriesName::new(s.name).map_err(ConfigError::Name)?,
                pins,
            });
        }
        Ok(Self {
            config_path,
            defaults,
            projects,
            series,
        })
    }

    pub fn project(&self, name: &ProjectName) -> Result<&Project, ConfigError> {
        self.projects
            .iter()
            .find(|p| &p.name == name)
            .ok_or_else(|| ConfigError::UnknownProject(name.to_string()))
    }

    pub fn series_by_name(&self, name: &SeriesName) -> Result<&Series, ConfigError> {
        self.series
            .iter()
            .find(|s| &s.name == name)
            .ok_or_else(|| ConfigError::UnknownSeries(name.to_string()))
    }

    /// Projects configured globally but not pinned by `series`,
    /// sorted alphabetically.
    pub fn projects_missing_from_series(&self, series: &Series) -> Vec<ProjectName> {
        let pinned: BTreeSet<&ProjectName> = series.pins.iter().map(|p| &p.project).collect();
        let mut missing: Vec<ProjectName> = self
            .projects
            .iter()
            .map(|p| &p.name)
            .filter(|n| !pinned.contains(*n))
            .cloned()
            .collect();
        missing.sort();
        missing
    }

    /// Like [`series_by_name`](Self::series_by_name) but also emits a
    /// `tracing::warn!` per configured project the series does not pin.
    pub fn series_by_name_with_coverage_check(
        &self,
        name: &SeriesName,
    ) -> Result<&Series, ConfigError> {
        let series = self.series_by_name(name)?;
        let missing = self.projects_missing_from_series(series);
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(ProjectName::as_str).collect();
            tracing::warn!(
                series = %name,
                missing_projects = ?names,
                "series {} pins {} project(s); these configured projects have no pin: {}. Was this intentional?",
                name, series.pins.len(), names.join(", ")
            );
        }
        Ok(series)
    }

    pub fn snapshot_dir(&self) -> PathBuf {
        let dir = &self.defaults.snapshot_dir;
        if dir.is_absolute() {
            dir.clone()
        } else {
            self.config_path
                .parent()
                .map(|p| p.join(dir))
                .unwrap_or_else(|| dir.clone())
        }
    }
}