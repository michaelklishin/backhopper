// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper.toml` schema and loader.

mod path_translation;

pub use path_translation::{
    PathTranslation, PathTranslations, TranslationDirection, TranslationOrigin,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::{ConfigError, NameError};
use crate::model::names::{ApplicationName, GitRef, ProjectName, SeriesName, TagGlob, TagName};
use crate::model::pin::{self, Pin, PinSelect, PinSpec};
use crate::store::SnapshotStore;
use crate::suites::rules::validate_template_placeholders;
use crate::suites::{ExtraRule, ExtraRuleTrigger, LineMatch};

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

    #[serde(default)]
    pub cache: CacheRaw,

    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectRaw>,

    #[serde(default, rename = "series")]
    pub series: Vec<SeriesRaw>,

    #[serde(default, rename = "suite_rule")]
    pub suite_rules: Vec<SuiteRuleRaw>,

    #[serde(default, rename = "path_translation")]
    pub path_translations: Vec<PathTranslationRaw>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathTranslationRaw {
    pub name: String,
    #[serde(default = "default_translation_direction")]
    pub direction: String,
    pub source_prefix: String,
    pub target_prefix: String,
}

fn default_translation_direction() -> String {
    "source_to_target".to_owned()
}

/// TOML form of a path-pattern suite rule. Accepts `include_suite` as a
/// single string or a list of strings; both end up in
/// `ExtraRule::include_suite_templates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteRuleRaw {
    pub name: Option<String>,
    pub when_modified_path_matches: String,
    #[serde(default)]
    pub when_modified_line_matches: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub include_suite: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub also: Vec<String>,
    #[serde(default)]
    pub include_suite_for_dep_modules: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

fn deserialize_string_or_vec<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<StringOrVec>::deserialize(d).map(|opt| match opt {
        None => Vec::new(),
        Some(StringOrVec::One(s)) => vec![s],
        Some(StringOrVec::Many(v)) => v,
    })
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

/// TOML form of the `[cache]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheRaw {
    pub ttl_days: Option<u32>,
}

/// Expiration policy for the workspace's on-disk caches. Time-based
/// expiry is the whole policy: no LRU, no size caps. Expiration is
/// hygiene, never correctness — the cache key components own
/// correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheSettings {
    /// Entries older than this many days expire. `0` disables expiry
    /// (`cache prune` and `cache clear` still work). The default is
    /// sized to RabbitMQ's monthly patch-release cadence: the real
    /// reuse window is one round warming the next, four to six weeks.
    pub ttl_days: u32,
}

pub const DEFAULT_CACHE_TTL_DAYS: u32 = 42;

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            ttl_days: DEFAULT_CACHE_TTL_DAYS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRaw {
    pub name: String,
    pub git_url: Option<String>,
    pub kind: Option<String>,
    pub family: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    /// Snapshots come from cloning `git_url` and walking its tags. The default.
    External,
    /// Snapshots are produced from the working repo at `--repo-dir-path`, at
    /// a `git_ref` resolved per pin. There can be at most one self-project.
    SelfRepo,
}

impl ProjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::External => "external",
            Self::SelfRepo => "self",
        }
    }
}

impl FromStr for ProjectKind {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "external" => Ok(Self::External),
            "self" => Ok(Self::SelfRepo),
            other => Err(ConfigError::UnknownProjectKind(other.to_owned())),
        }
    }
}

/// Which family-specific detectors run on top of the generic surface
/// checks. Declared per-project as `family = "ra"`. Orthogonal to
/// `ProjectKind` and `ProjectLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectFamily {
    /// No specialization. The default.
    #[default]
    Generic,
    /// OTP itself.
    ErlangOtp,
    /// `rabbitmq/ra`: Raft consensus. Wire-bearing version macros in
    /// `ra_log_segment`, `ra_log_wal`, `ra_log_snapshot`, `ra_snapshot`.
    Ra,
    /// `rabbitmq/osiris`: streaming log. Version constants in the
    /// segment and index file headers.
    Osiris,
    /// `rabbitmq/khepri`: metadata store. Node payload version constants.
    Khepri,
    /// `rabbitmq/rabbitmq-server`: implements `ra_machine` via
    /// `rabbit_fifo` and `rabbit_stream_coordinator`; carries `.schema`
    /// files; uses `?LOG_*`, `rabbit_log:*`, and `rabbit_khepri`.
    Rabbitmq,
}

impl ProjectFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::ErlangOtp => "erlang_otp",
            Self::Ra => "ra",
            Self::Osiris => "osiris",
            Self::Khepri => "khepri",
            Self::Rabbitmq => "rabbitmq",
        }
    }
}

impl FromStr for ProjectFamily {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "generic" => Ok(Self::Generic),
            "erlang_otp" => Ok(Self::ErlangOtp),
            "ra" => Ok(Self::Ra),
            "osiris" => Ok(Self::Osiris),
            "khepri" => Ok(Self::Khepri),
            "rabbitmq" => Ok(Self::Rabbitmq),
            other => Err(ConfigError::UnknownProjectFamily(other.to_owned())),
        }
    }
}

impl ProjectFamily {
    /// Wire-bearing macros, versioned-machine behaviours, and versioned-machine
    /// implementer modules this family declares.
    pub fn defaults(self) -> FamilyDefaults {
        match self {
            Self::Generic => FamilyDefaults::default(),
            Self::ErlangOtp => FamilyDefaults::default(),
            Self::Ra => FamilyDefaults {
                wire_constants: vec![
                    WireConstantDecl::new("ra_log_segment", &["VERSION", "MAGIC"]),
                    WireConstantDecl::new("ra_log_wal", &["CURRENT_VERSION", "MAGIC"]),
                    WireConstantDecl::new("ra_log_snapshot", &["VERSION", "MAGIC"]),
                    WireConstantDecl::new("ra_snapshot", &["IDX_VERSION", "IDX_MAGIC"]),
                    WireConstantDecl::new("ra", &["RA_PROTO_VERSION"]),
                ],
                versioned_machines: vec![VersionedMachineDecl {
                    behaviour: "ra_machine".into(),
                }],
                versioned_machine_impls: Vec::new(),
                test_helper_search_paths: Vec::new(),
                sibling_drift_vocabulary: Vec::new(),
            },
            Self::Osiris => FamilyDefaults {
                wire_constants: vec![WireConstantDecl::new(
                    "osiris",
                    &[
                        "MAGIC",
                        "VERSION",
                        "IDX_VERSION",
                        "LOG_VERSION",
                        "IDX_HEADER",
                        "LOG_HEADER",
                    ],
                )],
                ..Default::default()
            },
            Self::Khepri => FamilyDefaults {
                wire_constants: vec![WireConstantDecl::new(
                    "khepri_node",
                    &["INIT_DATA_VERSION", "INIT_CHILD_LIST_VERSION"],
                )],
                ..Default::default()
            },
            Self::Rabbitmq => FamilyDefaults {
                wire_constants: Vec::new(),
                versioned_machines: Vec::new(),
                versioned_machine_impls: vec![
                    VersionedMachineImplDecl {
                        module: "rabbit_fifo".into(),
                        version_function: "version".into(),
                        allow_state_flag_gating: true,
                    },
                    VersionedMachineImplDecl {
                        module: "rabbit_stream_coordinator".into(),
                        version_function: "version".into(),
                        allow_state_flag_gating: false,
                    },
                ],
                test_helper_search_paths: rabbitmq_default_test_helper_search_paths(),
                sibling_drift_vocabulary: rabbitmq_default_sibling_drift_vocabulary(),
            },
        }
    }
}

/// Per-family relative-path globs the test-module resolver scans when
/// looking up `helper_module:f/n` references in `_SUITE.erl` files.
/// Globs are matched against `RelativePath` strings on the target
/// tree; the first `*` segment expands to the application directory
/// name (e.g. `deps/rabbit`, `deps/amqp_client`). Empty for
/// `Generic`, so non-RabbitMQ projects do not get surprise verdict
/// changes from a feature they did not opt into.
fn rabbitmq_default_test_helper_search_paths() -> Vec<String> {
    vec![
        "deps/*/test".to_owned(),
        "deps/*/src".to_owned(),
        "deps/rabbitmq_ct_helpers/src".to_owned(),
    ]
}

/// Subject-prefilter terms `siblings doctor` matches against
/// first-parent commit subjects when ranking sibling-branch fixes
/// that should have cascaded. Empty for `Generic`, so only RabbitMQ
/// projects opt into the heuristic by default. Calibrated against the
/// labeled corpus in
/// `backhopper-cli/tests/fixtures/sibling_drift_corpus/`; the F1 gate
/// there fails the build if an edit here regresses the scorer.
fn rabbitmq_default_sibling_drift_vocabulary() -> Vec<String> {
    [
        "flake",
        "crash",
        "find_crashes",
        "ignored_crashes",
        "await",
        "wait_for",
        "assertMatch",
        "_SUITE",
        "deflake",
        "race",
        "retry",
        "wait_until",
        "eventually",
        "polling",
        "flaky",
        "intermittent",
        "mixed version",
        "fix a test",
        "fix test",
        "test fix",
        "fragile",
        "expected exception",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FamilyDefaults {
    pub wire_constants: Vec<WireConstantDecl>,
    pub versioned_machines: Vec<VersionedMachineDecl>,
    pub versioned_machine_impls: Vec<VersionedMachineImplDecl>,
    /// See `rabbitmq_default_test_helper_search_paths` for the
    /// `ProjectFamily::Rabbitmq` default and the glob convention.
    pub test_helper_search_paths: Vec<String>,
    /// See `rabbitmq_default_sibling_drift_vocabulary`. Consumed by
    /// `siblings doctor`'s subject prefilter.
    pub sibling_drift_vocabulary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireConstantDecl {
    pub module: String,
    pub macros: Vec<String>,
}

impl WireConstantDecl {
    pub fn new(module: &str, macros: &[&str]) -> Self {
        Self {
            module: module.to_owned(),
            macros: macros.iter().map(|m| (*m).to_owned()).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedMachineDecl {
    pub behaviour: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedMachineImplDecl {
    pub module: String,
    pub version_function: String,
    pub allow_state_flag_gating: bool,
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
    SelfBranch {
        project: String,
        branch: String,
        #[serde(default)]
        repo_dir_path: Option<PathBuf>,
    },
    SelfSha {
        project: String,
        sha: String,
        #[serde(default)]
        repo_dir_path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Defaults {
    pub snapshot_dir: PathBuf,
    pub fallback_branch: String,
    pub scan_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Erlang,
    Elixir,
}

impl Language {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Erlang => "erlang",
            Self::Elixir => "elixir",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
    /// `None` for self-projects; the repo is the runtime `--repo-dir-path`.
    pub git_url: Option<PathBuf>,
    pub kind: ProjectKind,
    pub family: ProjectFamily,
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

    /// `git_url` is only present for external projects. Returns a clear
    /// error when callers ask for it on a self-project.
    pub fn require_git_url(&self) -> Result<&Path, ConfigError> {
        self.git_url
            .as_deref()
            .ok_or_else(|| ConfigError::SelfProjectHasNoGitUrl(self.name.to_string()))
    }

    pub fn is_self(&self) -> bool {
        matches!(self.kind, ProjectKind::SelfRepo)
    }

    /// Prefix-matches `path` against `scan_paths` and `app_roots`,
    /// stripping glob suffixes.
    pub fn owns_path(&self, path: &Path) -> bool {
        let candidate = path.to_string_lossy();
        self.scan_paths
            .iter()
            .chain(self.app_roots.iter())
            .any(|raw| path_under_glob_prefix(&candidate, raw))
    }
}

fn path_under_glob_prefix(candidate: &str, raw: &str) -> bool {
    let prefix = glob_directory_prefix(raw).trim_end_matches('/');
    if prefix.is_empty() {
        return false;
    }
    if !candidate.starts_with(prefix) {
        return false;
    }
    let rest = &candidate[prefix.len()..];
    rest.is_empty() || rest.starts_with('/')
}

// Return the literal directory prefix before any glob metacharacter
fn glob_directory_prefix(pattern: &str) -> &str {
    let metachars = ['*', '?', '[', '{'];
    match pattern.find(metachars) {
        Some(i) => {
            let head = &pattern[..i];
            match head.rfind('/') {
                Some(slash) => &head[..=slash],
                None => "",
            }
        }
        None => pattern,
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
    #[serde(default)]
    pub cache: CacheSettings,
    pub projects: Vec<Project>,
    pub series: Vec<Series>,
    pub suite_rules: Vec<ExtraRule>,
    pub path_translations: PathTranslations,
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
        let cache = CacheSettings {
            ttl_days: raw.cache.ttl_days.unwrap_or(DEFAULT_CACHE_TTL_DAYS),
        };
        let mut projects = Vec::with_capacity(raw.projects.len());
        for p in raw.projects {
            projects.push(parse_project(p, &defaults)?);
        }
        let self_projects: Vec<String> = projects
            .iter()
            .filter(|p| p.kind == ProjectKind::SelfRepo)
            .map(|p| p.name.to_string())
            .collect();
        if self_projects.len() > 1 {
            return Err(ConfigError::MultipleSelfProjects {
                projects: self_projects,
            });
        }
        let project_kinds: BTreeMap<&ProjectName, ProjectKind> =
            projects.iter().map(|p| (&p.name, p.kind)).collect();
        let mut series = Vec::with_capacity(raw.series.len());
        for s in raw.series {
            let mut pins = Vec::with_capacity(s.pins.len());
            for pin in s.pins {
                let spec = parse_pin(pin)?;
                let Some(kind) = project_kinds.get(spec.project()).copied() else {
                    return Err(ConfigError::SeriesPinsUnknownProject {
                        series: s.name.clone(),
                        project: spec.project().to_string(),
                    });
                };
                if spec.is_self() && kind != ProjectKind::SelfRepo {
                    return Err(ConfigError::SelfPinReferencesExternalProject {
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
        let mut suite_rules = Vec::with_capacity(raw.suite_rules.len());
        for (idx, r) in raw.suite_rules.into_iter().enumerate() {
            suite_rules.push(parse_suite_rule(idx, r)?);
        }
        let path_translations = PathTranslations::from_config_stanzas(raw.path_translations)?;
        Ok(Self {
            config_path,
            defaults,
            cache,
            projects,
            series,
            suite_rules,
            path_translations,
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

fn parse_suite_rule(idx: usize, raw: SuiteRuleRaw) -> Result<ExtraRule, ConfigError> {
    let pattern = raw.when_modified_path_matches;
    let compiled = Regex::new(&pattern).map_err(|e| ConfigError::SuiteRuleRegex {
        rule_index: idx,
        detail: e.to_string(),
    })?;
    let path_capture_names: Vec<String> = compiled
        .capture_names()
        .flatten()
        .map(|s| s.to_owned())
        .collect();
    let line_match = match raw.when_modified_line_matches {
        Some(line_pattern) => {
            let line_compiled =
                Regex::new(&line_pattern).map_err(|e| ConfigError::SuiteRuleRegex {
                    rule_index: idx,
                    detail: e.to_string(),
                })?;
            let line_captures: Vec<String> = line_compiled
                .capture_names()
                .flatten()
                .map(|s| s.to_owned())
                .collect();
            Some(LineMatch {
                pattern: line_pattern,
                captures: line_captures,
            })
        }
        None => None,
    };
    let mut allowed_captures: Vec<String> = path_capture_names.clone();
    if let Some(lm) = &line_match {
        allowed_captures.extend(lm.captures.iter().cloned());
    }
    let mut templates: Vec<String> = Vec::new();
    templates.extend(raw.include_suite);
    templates.extend(raw.also);
    for t in &templates {
        if let Err(e) = validate_template_placeholders(t, &allowed_captures) {
            return Err(ConfigError::SuiteRuleUnknownPlaceholder {
                rule_index: idx,
                placeholder: e.placeholder,
            });
        }
    }
    if raw.include_suite_for_dep_modules && !allowed_captures.iter().any(|c| c == "dep") {
        return Err(ConfigError::SuiteRuleMissingDepCapture { rule_index: idx });
    }
    let name = raw.name.unwrap_or_else(|| format!("suite_rule_{idx}"));
    Ok(ExtraRule {
        name,
        trigger: ExtraRuleTrigger::PathRegex {
            pattern,
            captures: path_capture_names,
        },
        include_suites: Vec::new(),
        include_suite_templates: templates,
        line_match,
        include_suite_for_dep_modules: raw.include_suite_for_dep_modules,
    })
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
    let kind = p
        .kind
        .as_deref()
        .map(ProjectKind::from_str)
        .transpose()?
        .unwrap_or(ProjectKind::External);
    let family = p
        .family
        .as_deref()
        .map(ProjectFamily::from_str)
        .transpose()?
        .unwrap_or_default();
    let git_url = match (kind, p.git_url) {
        (ProjectKind::External, Some(g)) => Some(PathBuf::from(g)),
        (ProjectKind::External, None) => {
            return Err(ConfigError::ExternalProjectMissingGitUrl(name.to_string()));
        }
        (ProjectKind::SelfRepo, None) => None,
        (ProjectKind::SelfRepo, Some(_)) => {
            return Err(ConfigError::SelfProjectHasGitUrl(name.to_string()));
        }
    };
    Ok(Project {
        name,
        git_url,
        kind,
        family,
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
        PinRaw::SelfBranch {
            project,
            branch,
            repo_dir_path,
        } => {
            let project = ProjectName::new(project).map_err(ConfigError::Name)?;
            let git_ref = GitRef::new(branch).map_err(ConfigError::Name)?;
            Ok(PinSpec::SelfRef {
                project,
                git_ref,
                repo_dir_path,
            })
        }
        PinRaw::SelfSha {
            project,
            sha,
            repo_dir_path,
        } => {
            let project = ProjectName::new(project).map_err(ConfigError::Name)?;
            let git_ref = GitRef::new(sha).map_err(ConfigError::Name)?;
            Ok(PinSpec::SelfRef {
                project,
                git_ref,
                repo_dir_path,
            })
        }
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
