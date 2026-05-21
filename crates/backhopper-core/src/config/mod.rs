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
use crate::model::names::{ApplicationName, ProjectName, SeriesName, TagGlob, TagName};
use crate::model::pin::{self, Pin, PinSelect, PinSpec};
use crate::store::SnapshotStore;

pub const CONFIG_VERSION: u32 = 1;

const DEFAULT_SCAN_PATHS: &[&str] = &["src/**/*.erl", "src/**/*.ex", "include/**/*.hrl"];
const DEFAULT_SNAPSHOT_DIR: &str = "snapshots";
const DEFAULT_FALLBACK_BRANCH: &str = "main";

const ERLANG_OTP_APP_ROOTS: &[&str] = &["lib/*", "erts/preloaded"];
const ERLANG_OTP_EXCLUDE_APPS: &[&str] = &[
    "odbc",
    "snmp",
    "ssh",
    "tftp",
    "ftp",
    "wx",
    "megaco",
    "edoc",
    "jinterface",
    "diameter",
];
const ERLANG_OTP_EXCLUDED_SUBDIRS: &[&str] = &["doc", "example", "examples", "test"];
const ERLANG_OTP_TAG_PATTERN: &str = "OTP-*";
const ERLANG_OTP_MIN_TAG: &str = "OTP-26.0";

/// Substrings that mark a tag as a pre-release. Tags containing any of these
/// are excluded by default at snapshot-generation time.
const DEFAULT_EXCLUDE_TAG_MARKERS: &[&str] = &["-rc", "-alpha", "-beta", "-pre"];

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
    pub layout: Option<String>,
    pub app_roots: Option<Vec<String>>,
    pub include_apps: Option<Vec<String>>,
    pub exclude_apps: Option<Vec<String>>,
    pub excluded_subdirs: Option<Vec<String>>,
    pub tag_pattern: Option<String>,
    #[serde(alias = "oldest_tag")]
    pub min_tag: Option<String>,
    pub exclude_tag_markers: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesRaw {
    pub name: String,
    pub pins: Vec<PinRaw>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum PinRaw {
    Literal {
        project: String,
        tag: String,
    },
    Pattern {
        project: String,
        tag_pattern: String,
        select: String,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLayout {
    SingleApp,
    MultiApp,
    ErlangOtp,
}

impl ProjectLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleApp => "single_app",
            Self::MultiApp => "multi_app",
            Self::ErlangOtp => "erlang_otp",
        }
    }

    pub fn defaults(self) -> LayoutDefaults {
        match self {
            Self::SingleApp | Self::MultiApp => LayoutDefaults::default(),
            Self::ErlangOtp => LayoutDefaults {
                app_roots: ERLANG_OTP_APP_ROOTS
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                include_apps: Vec::new(),
                exclude_apps: ERLANG_OTP_EXCLUDE_APPS
                    .iter()
                    .map(|s| ApplicationName::new(*s).expect("static name"))
                    .collect(),
                excluded_subdirs: ERLANG_OTP_EXCLUDED_SUBDIRS
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                tag_pattern: Some(TagGlob::new(ERLANG_OTP_TAG_PATTERN).expect("static tag glob")),
                min_tag: Some(TagName::new(ERLANG_OTP_MIN_TAG).expect("static tag")),
            },
        }
    }
}

impl FromStr for ProjectLayout {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "single_app" => Ok(Self::SingleApp),
            "multi_app" => Ok(Self::MultiApp),
            "erlang_otp" => Ok(Self::ErlangOtp),
            other => Err(ConfigError::UnknownProjectLayout(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutDefaults {
    pub app_roots: Vec<String>,
    pub include_apps: Vec<ApplicationName>,
    pub exclude_apps: Vec<ApplicationName>,
    pub excluded_subdirs: Vec<String>,
    pub tag_pattern: Option<TagGlob>,
    pub min_tag: Option<TagName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: ProjectName,
    pub git_url: PathBuf,
    pub language: Language,
    pub tag_prefix: String,
    pub public_modules: Vec<String>,
    pub internal_modules: Vec<String>,
    pub layout: ProjectLayout,
    pub scan_paths: Vec<String>,
    pub app_roots: Vec<String>,
    pub include_apps: Vec<ApplicationName>,
    pub exclude_apps: Vec<ApplicationName>,
    pub excluded_subdirs: Vec<String>,
    pub tag_pattern: Option<TagGlob>,
    pub min_tag: Option<TagName>,
    pub exclude_tag_markers: Vec<String>,
}

impl Project {
    /// True if `tag` contains any of this project's pre-release markers.
    pub fn is_prerelease_tag(&self, tag: &TagName) -> bool {
        self.exclude_tag_markers
            .iter()
            .any(|marker| tag.as_str().contains(marker))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    pub name: SeriesName,
    pub pins: Vec<PinSpec>,
}

impl Series {
    /// Resolve every pin spec against `store`, returning concrete `Pin`s.
    pub fn resolve_pins<M>(&self, store: &SnapshotStore<M>) -> Result<Vec<Pin>, ConfigError> {
        pin::resolve_all(&self.pins, store)
    }
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
            projects.push(parse_project(p, &defaults)?);
        }
        let project_names: BTreeSet<&ProjectName> = projects.iter().map(|p| &p.name).collect();
        let mut series = Vec::with_capacity(raw.series.len());
        for s in raw.series {
            let mut pins = Vec::with_capacity(s.pins.len());
            for pin in s.pins {
                let spec = parse_pin(pin)?;
                if !project_names.contains(spec.project()) {
                    return Err(ConfigError::SeriesPinsUnknownProject {
                        series: s.name.clone(),
                        project: spec.project().to_string(),
                    });
                }
                pins.push(spec);
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

    /// Projects configured globally but not pinned by `series`, sorted alphabetically.
    pub fn projects_missing_from_series(&self, series: &Series) -> Vec<ProjectName> {
        let pinned: BTreeSet<&ProjectName> = series.pins.iter().map(|p| p.project()).collect();
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

fn parse_project(p: ProjectRaw, defaults: &Defaults) -> Result<Project, ConfigError> {
    let layout = p
        .layout
        .as_deref()
        .map(ProjectLayout::from_str)
        .transpose()?
        .unwrap_or(ProjectLayout::SingleApp);
    let language = p
        .language
        .as_deref()
        .map(Language::from_str)
        .transpose()?
        .unwrap_or(Language::Erlang);
    let layout_defaults = layout.defaults();
    let scan_paths = p.scan_paths.unwrap_or_else(|| match layout {
        ProjectLayout::SingleApp => defaults.scan_paths.clone(),
        ProjectLayout::MultiApp | ProjectLayout::ErlangOtp => Vec::new(),
    });
    let app_roots = p
        .app_roots
        .unwrap_or_else(|| layout_defaults.app_roots.clone());
    let include_apps = match p.include_apps {
        Some(v) => parse_app_names(v)?,
        None => layout_defaults.include_apps.clone(),
    };
    let exclude_apps = match p.exclude_apps {
        Some(v) => parse_app_names(v)?,
        None => layout_defaults.exclude_apps.clone(),
    };
    let excluded_subdirs = p
        .excluded_subdirs
        .unwrap_or_else(|| layout_defaults.excluded_subdirs.clone());
    let tag_pattern = match p.tag_pattern {
        Some(s) => Some(TagGlob::new(s).map_err(ConfigError::Name)?),
        None => layout_defaults.tag_pattern.clone(),
    };
    let min_tag = match p.min_tag {
        Some(s) => Some(TagName::new(s).map_err(ConfigError::Name)?),
        None => layout_defaults.min_tag.clone(),
    };
    let exclude_tag_markers = p.exclude_tag_markers.unwrap_or_else(|| {
        DEFAULT_EXCLUDE_TAG_MARKERS
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    });
    let name = ProjectName::new(p.name).map_err(ConfigError::Name)?;
    if matches!(layout, ProjectLayout::MultiApp | ProjectLayout::ErlangOtp) && app_roots.is_empty()
    {
        return Err(ConfigError::LayoutWithoutAppRoots {
            project: name.to_string(),
            layout: layout.as_str().to_owned(),
        });
    }
    Ok(Project {
        name,
        git_url: PathBuf::from(p.git_url),
        language,
        tag_prefix: p.tag_prefix.unwrap_or_else(|| String::from("v")),
        public_modules: p.public_modules.unwrap_or_default(),
        internal_modules: p.internal_modules.unwrap_or_default(),
        layout,
        scan_paths,
        app_roots,
        include_apps,
        exclude_apps,
        excluded_subdirs,
        tag_pattern,
        min_tag,
        exclude_tag_markers,
    })
}

fn parse_app_names(raw: Vec<String>) -> Result<Vec<ApplicationName>, ConfigError> {
    raw.into_iter()
        .map(|s| ApplicationName::new(s).map_err(ConfigError::Name))
        .collect()
}

fn parse_pin(pin: PinRaw) -> Result<PinSpec, ConfigError> {
    match pin {
        PinRaw::Literal { project, tag } => {
            let project = ProjectName::new(project).map_err(ConfigError::Name)?;
            let tag = TagName::new(tag).map_err(ConfigError::Name)?;
            Ok(PinSpec::literal(project, tag))
        }
        PinRaw::Pattern {
            project,
            tag_pattern,
            select,
        } => {
            let project = ProjectName::new(project).map_err(ConfigError::Name)?;
            let pattern = TagGlob::new(tag_pattern).map_err(ConfigError::Name)?;
            let select = match select.as_str() {
                "latest" => PinSelect::Latest,
                "oldest" => PinSelect::Oldest,
                other => return Err(ConfigError::PinUnknownSelect(other.to_owned())),
            };
            Ok(PinSpec::pattern(project, pattern, select))
        }
    }
}
