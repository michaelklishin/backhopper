// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `[[host]]`: the Erlang system plugins are loaded into, its release
//! tags, pin source, release roots and supported Erlang ranges.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{Project, ProjectLayout};
use crate::errors::ConfigError;
use crate::model::names::{
    ApplicationName, DependencyName, DependencyVersion, HostName, MakeVariableName, ProjectName,
    RelativePath, SeriesTagPattern, vocabulary,
};
use crate::model::non_empty::NonEmpty;
use crate::model::release_roots::{MakefileVariable, ReleaseRoots};
use crate::model::release_version::{
    ErlangMajor, ErlangVersionRange, HostVersion, ReleaseVersionError,
};

pub const ERLANG_OTP_TAG_PREFIX: &str = "OTP-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRaw {
    pub name: String,
    pub project: String,
    pub otp_project: String,
    pub series_tag_pattern: String,
    pub erlang_majors: Vec<u32>,
    pub pins: HostPinsRaw,
    pub release_roots: ReleaseRootsRaw,
    #[serde(default)]
    pub erlang_support: Option<ErlangSupportRaw>,
    #[serde(default)]
    pub erlang_support_file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPinsRaw {
    pub source: String,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub releases: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRootsRaw {
    #[serde(default)]
    pub apps: Option<Vec<String>>,
    #[serde(default)]
    pub makefile_variable: Option<MakefileVariableRaw>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MakefileVariableRaw {
    pub file_path: String,
    pub variable: String,
}

/// The two tables of `[host.erlang_support]`, also the whole content of
/// an `erlang_support_file_path` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErlangSupportRaw {
    pub default: String,
    #[serde(default)]
    pub releases: BTreeMap<String, String>,
}

vocabulary!(
    pub enum PinSourceKind: "pin source" {
        ErlangMk => "erlang_mk",
        Table => "table",
    }
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PinSource {
    ErlangMk { file_path: RelativePath },
    Table(BTreeMap<HostVersion, BTreeMap<DependencyName, DependencyVersion>>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErlangSupport {
    default: ErlangVersionRange,
    releases: BTreeMap<HostVersion, ErlangVersionRange>,
}

impl ErlangSupport {
    #[must_use]
    pub fn new(
        default: ErlangVersionRange,
        releases: BTreeMap<HostVersion, ErlangVersionRange>,
    ) -> Self {
        Self { default, releases }
    }

    /// The entry of the nearest listed release at or below `release`, and
    /// `default` for anything below the first entry.
    #[must_use]
    pub fn range_for(&self, release: &HostVersion) -> &ErlangVersionRange {
        self.releases
            .range(..=release)
            .next_back()
            .map_or(&self.default, |(_, range)| range)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Host {
    pub name: HostName,
    pub project: ProjectName,
    pub otp_project: ProjectName,
    pub series_tag_pattern: SeriesTagPattern,
    pub erlang_majors: NonEmpty<ErlangMajor>,
    pub pins: PinSource,
    pub release_roots: ReleaseRoots,
    pub erlang_support: ErlangSupport,
}

#[derive(Debug, Error)]
pub enum HostConfigError {
    #[error("host {host}: project {project:?} is not configured")]
    UnknownProject { host: String, project: String },

    #[error("host {host}: project {project} is the self project: host releases are tags")]
    ProjectIsSelf { host: String, project: String },

    #[error("host {host}: otp_project {project} must have layout = \"erlang_otp\"")]
    OtpProjectLayout { host: String, project: String },

    #[error(
        "host {host}: otp_project {project} has tag_prefix {prefix:?}: it must be {ERLANG_OTP_TAG_PREFIX:?}"
    )]
    OtpProjectTagPrefix {
        host: String,
        project: String,
        prefix: String,
    },

    #[error("host {host}: duplicate host name")]
    Duplicate { host: String },

    #[error("host {host}: erlang_majors must list at least one major")]
    NoErlangMajors { host: String },

    #[error("host {host}: erlang_majors: {source}")]
    ErlangMajor {
        host: String,
        source: ReleaseVersionError,
    },

    #[error("host {host}: pins: {detail}")]
    Pins { host: String, detail: String },

    #[error("host {host}: release_roots: {detail}")]
    ReleaseRoots { host: String, detail: &'static str },

    #[error("host {host}: erlang_support and erlang_support_file_path are mutually exclusive")]
    ErlangSupportBoth { host: String },

    #[error("host {host}: one of erlang_support and erlang_support_file_path is required")]
    ErlangSupportNeither { host: String },

    #[error("host {host}: erlang support {location}: {source}")]
    ErlangSupportEntry {
        host: String,
        location: String,
        source: ReleaseVersionError,
    },

    #[error("host {host}: erlang_support_file_path {path}: {detail}")]
    ErlangSupportFile {
        host: String,
        path: String,
        detail: String,
    },
}

impl Host {
    pub(super) fn resolve(
        raw: HostRaw,
        config_dir: &Path,
        projects: &[Project],
    ) -> Result<Self, ConfigError> {
        let host = raw.name.clone();
        let name = HostName::new(raw.name)?;
        let project = ProjectName::new(raw.project)?;
        let otp_project = ProjectName::new(raw.otp_project)?;
        check_host_project(&host, &project, projects)?;
        check_otp_project(&host, &otp_project, projects)?;
        let series_tag_pattern = SeriesTagPattern::new(raw.series_tag_pattern)?;
        let erlang_majors = resolve_majors(&host, raw.erlang_majors)?;
        let pins = resolve_pins(&host, raw.pins)?;
        let release_roots = resolve_release_roots(&host, raw.release_roots)?;
        let erlang_support = resolve_erlang_support(
            &host,
            raw.erlang_support,
            raw.erlang_support_file_path,
            config_dir,
        )?;
        Ok(Self {
            name,
            project,
            otp_project,
            series_tag_pattern,
            erlang_majors,
            pins,
            release_roots,
            erlang_support,
        })
    }
}

fn find_project<'p>(
    host: &str,
    name: &ProjectName,
    projects: &'p [Project],
) -> Result<&'p Project, HostConfigError> {
    projects
        .iter()
        .find(|p| &p.name == name)
        .ok_or_else(|| HostConfigError::UnknownProject {
            host: host.to_owned(),
            project: name.to_string(),
        })
}

fn check_host_project(
    host: &str,
    name: &ProjectName,
    projects: &[Project],
) -> Result<(), HostConfigError> {
    if find_project(host, name, projects)?.is_self() {
        return Err(HostConfigError::ProjectIsSelf {
            host: host.to_owned(),
            project: name.to_string(),
        });
    }
    Ok(())
}

fn check_otp_project(
    host: &str,
    name: &ProjectName,
    projects: &[Project],
) -> Result<(), HostConfigError> {
    let project = find_project(host, name, projects)?;
    if project.layout != ProjectLayout::ErlangOtp {
        return Err(HostConfigError::OtpProjectLayout {
            host: host.to_owned(),
            project: name.to_string(),
        });
    }
    if project.tag_prefix != ERLANG_OTP_TAG_PREFIX {
        return Err(HostConfigError::OtpProjectTagPrefix {
            host: host.to_owned(),
            project: name.to_string(),
            prefix: project.tag_prefix.clone(),
        });
    }
    Ok(())
}

fn resolve_majors(host: &str, raw: Vec<u32>) -> Result<NonEmpty<ErlangMajor>, HostConfigError> {
    let majors = raw
        .into_iter()
        .map(ErlangMajor::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| HostConfigError::ErlangMajor {
            host: host.to_owned(),
            source,
        })?;
    let mut majors = NonEmpty::try_from(majors).map_err(|_| HostConfigError::NoErlangMajors {
        host: host.to_owned(),
    })?;
    majors.sort_dedup();
    Ok(majors)
}

fn resolve_pins(host: &str, raw: HostPinsRaw) -> Result<PinSource, ConfigError> {
    let pins_error = |detail: String| HostConfigError::Pins {
        host: host.to_owned(),
        detail,
    };
    match PinSourceKind::from_str(&raw.source)? {
        PinSourceKind::ErlangMk => {
            if raw.releases.is_some() {
                return Err(pins_error("releases is only for source = \"table\"".into()).into());
            }
            let file_path = raw
                .file_path
                .ok_or_else(|| pins_error("source = \"erlang_mk\" needs file_path".into()))?;
            Ok(PinSource::ErlangMk {
                file_path: RelativePath::new(file_path)?,
            })
        }
        PinSourceKind::Table => {
            if raw.file_path.is_some() {
                return Err(
                    pins_error("file_path is only for source = \"erlang_mk\"".into()).into(),
                );
            }
            let releases = raw
                .releases
                .filter(|releases| !releases.is_empty())
                .ok_or_else(|| {
                    pins_error("source = \"table\" needs at least one release".into())
                })?;
            let mut table = BTreeMap::new();
            for (release, deps) in releases {
                let version = HostVersion::from_str(&release)
                    .map_err(|e| pins_error(format!("release {release:?}: {e}")))?;
                let mut pins = BTreeMap::new();
                for (dep, dep_version) in deps {
                    pins.insert(
                        DependencyName::new(dep)?,
                        DependencyVersion::new(dep_version)?,
                    );
                }
                table.insert(version, pins);
            }
            Ok(PinSource::Table(table))
        }
    }
}

fn resolve_release_roots(host: &str, raw: ReleaseRootsRaw) -> Result<ReleaseRoots, ConfigError> {
    let roots_error = |detail: &'static str| HostConfigError::ReleaseRoots {
        host: host.to_owned(),
        detail,
    };
    let apps = match raw.apps {
        Some(apps) => {
            let apps = apps
                .into_iter()
                .map(ApplicationName::new)
                .collect::<Result<Vec<_>, _>>()?;
            Some(NonEmpty::try_from(apps).map_err(|_| roots_error("apps is empty"))?)
        }
        None => None,
    };
    let variable = raw
        .makefile_variable
        .map(|raw| {
            Ok::<_, ConfigError>(MakefileVariable {
                file_path: RelativePath::new(raw.file_path)?,
                variable: MakeVariableName::new(raw.variable)?,
            })
        })
        .transpose()?;
    match (apps, variable) {
        (Some(apps), None) => Ok(ReleaseRoots::Apps(apps)),
        (None, Some(variable)) => Ok(ReleaseRoots::Variable(variable)),
        (Some(apps), Some(variable)) => Ok(ReleaseRoots::Both { apps, variable }),
        (None, None) => Err(roots_error("needs apps, makefile_variable or both").into()),
    }
}

fn resolve_erlang_support(
    host: &str,
    inline: Option<ErlangSupportRaw>,
    file_path: Option<String>,
    config_dir: &Path,
) -> Result<ErlangSupport, ConfigError> {
    let tables = match (inline, file_path) {
        (Some(_), Some(_)) => {
            return Err(HostConfigError::ErlangSupportBoth {
                host: host.to_owned(),
            }
            .into());
        }
        (None, None) => {
            return Err(HostConfigError::ErlangSupportNeither {
                host: host.to_owned(),
            }
            .into());
        }
        (Some(tables), None) => tables,
        (None, Some(path)) => read_support_file(host, &path, config_dir)?,
    };
    let entry_error = |location: String| {
        let host = host.to_owned();
        move |source| HostConfigError::ErlangSupportEntry {
            host: host.clone(),
            location: location.clone(),
            source,
        }
    };
    let default =
        ErlangVersionRange::from_str(&tables.default).map_err(entry_error("default".to_owned()))?;
    let mut releases = BTreeMap::new();
    for (release, range) in &tables.releases {
        let location = format!("release {release:?}");
        let version = HostVersion::from_str(release).map_err(entry_error(location.clone()))?;
        let range = ErlangVersionRange::from_str(range).map_err(entry_error(location))?;
        releases.insert(version, range);
    }
    Ok(ErlangSupport::new(default, releases))
}

fn read_support_file(
    host: &str,
    path: &str,
    config_dir: &Path,
) -> Result<ErlangSupportRaw, HostConfigError> {
    let file_error = |detail: String| HostConfigError::ErlangSupportFile {
        host: host.to_owned(),
        path: path.to_owned(),
        detail,
    };
    let text = fs::read_to_string(config_dir.join(path)).map_err(|e| file_error(e.to_string()))?;
    toml::from_str(&text).map_err(|e| file_error(e.to_string()))
}
