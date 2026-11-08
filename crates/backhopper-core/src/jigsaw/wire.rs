// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::jigsaw::loaded::{SnapshotGap, StoreStatus};
use crate::jigsaw::plan::{HostRelease, JigsawPlan, PlanError, PlanParts, TagSourceKind};
use crate::jigsaw::releases::SeriesReleases;
use crate::jigsaw::tags::OtpLine;
use crate::model::names::{
    ApplicationName, CommitSha, DependencyName, ExtractorVersion, HostFactsDigest, HostName,
    ProjectName, TagName,
};
use crate::model::non_empty::NonEmpty;
use crate::model::pin::Pin;
use crate::model::release_version::{
    ErlangVersionRange, HostSeries, HostTag, OtpLineVersion, OtpTag,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawPlanPayload {
    pub host: String,
    pub host_project: String,
    pub otp_project: String,
    pub series: String,
    pub tag_source: String,
    pub extractor_version: String,
    pub host_facts_digest: String,
    pub releases: Vec<PlanReleasePayload>,
    pub otp_lines: Vec<PlanOtpLinePayload>,
    pub untracked_dependencies: Vec<String>,
    pub excluded_otp_apps: Vec<String>,
    pub required_snapshots: Vec<Pin>,
    pub missing_snapshots: Vec<MissingSnapshotPayload>,
    pub cache_key: String,
    pub restore_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PlanReleasePayload {
    pub tag: String,
    pub commit: String,
    pub pins: BTreeMap<String, String>,
    pub erlang_support: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PlanOtpLinePayload {
    pub line: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MissingSnapshotPayload {
    pub project: String,
    pub tag: String,
    pub reason: SnapshotGap,
    pub detail: Option<String>,
}

fn wire<E: Display>(field: &'static str) -> impl Fn(E) -> PlanError {
    move |error| PlanError::Wire {
        field,
        detail: error.to_string(),
    }
}

fn parse<T: FromStr>(field: &'static str, value: &str) -> Result<T, PlanError>
where
    T::Err: Display,
{
    value.parse().map_err(wire(field))
}

impl JigsawPlan {
    #[must_use]
    pub fn to_wire(&self, status: &StoreStatus) -> JigsawPlanPayload {
        let missing_snapshots = match status {
            StoreStatus::Stocked => Vec::new(),
            StoreStatus::Unstocked(unstocked) => unstocked
                .gaps()
                .into_iter()
                .map(|(pin, gap)| MissingSnapshotPayload {
                    project: pin.project.to_string(),
                    tag: pin.tag.to_string(),
                    reason: gap,
                    detail: unstocked.detail(&pin),
                })
                .collect(),
        };
        JigsawPlanPayload {
            host: self.host().to_string(),
            host_project: self.host_project().to_string(),
            otp_project: self.otp_project().to_string(),
            series: self.series().to_string(),
            tag_source: self.tag_source().to_string(),
            extractor_version: self.extractor_version().to_string(),
            host_facts_digest: self.host_facts().to_string(),
            releases: self
                .releases()
                .iter()
                .map(|release| PlanReleasePayload {
                    tag: release.tag.tag().to_string(),
                    commit: release.commit.to_string(),
                    pins: release
                        .pins
                        .iter()
                        .map(|(project, tag)| (project.to_string(), tag.to_string()))
                        .collect(),
                    erlang_support: release.erlang_support.to_string(),
                })
                .collect(),
            otp_lines: self
                .otp_lines()
                .iter()
                .map(|line| PlanOtpLinePayload {
                    line: line.line().to_string(),
                    tags: line
                        .tags()
                        .iter()
                        .map(|tag| tag.tag().to_string())
                        .collect(),
                })
                .collect(),
            untracked_dependencies: self
                .untracked_dependencies()
                .iter()
                .map(ToString::to_string)
                .collect(),
            excluded_otp_apps: self
                .excluded_otp_apps()
                .iter()
                .map(ToString::to_string)
                .collect(),
            required_snapshots: self.required().into_iter().collect(),
            missing_snapshots,
            cache_key: self.cache_key().to_string(),
            restore_key: self.cache_key().restore_prefix().to_owned(),
        }
    }

    /// The payload's `cache_key`, `restore_key`, `required_snapshots` and
    /// `missing_snapshots` are derived, so they are not read.
    pub fn from_wire(payload: &JigsawPlanPayload, config: &Config) -> Result<Self, PlanError> {
        let host_project_name: ProjectName = parse("host_project", &payload.host_project)?;
        let otp_project_name: ProjectName = parse("otp_project", &payload.otp_project)?;
        let host_project = config.project(&host_project_name)?;
        let otp_project = config.project(&otp_project_name)?;
        let releases = payload
            .releases
            .iter()
            .map(|release| {
                let tag: TagName = parse("releases.tag", &release.tag)?;
                let pins = release
                    .pins
                    .iter()
                    .map(|(project, tag)| {
                        Ok((
                            parse::<ProjectName>("releases.pins", project)?,
                            parse::<TagName>("releases.pins", tag)?,
                        ))
                    })
                    .collect::<Result<_, PlanError>>()?;
                Ok(HostRelease {
                    tag: HostTag::parse(host_project, tag).map_err(wire("releases.tag"))?,
                    commit: parse::<CommitSha>("releases.commit", &release.commit)?,
                    pins,
                    erlang_support: parse::<ErlangVersionRange>(
                        "releases.erlang_support",
                        &release.erlang_support,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let releases = SeriesReleases::new(releases)?;
        let series: HostSeries = parse("series", &payload.series)?;
        if releases.series() != series {
            return Err(PlanError::Wire {
                field: "series",
                detail: format!("releases are of series {}, not {series}", releases.series()),
            });
        }
        let otp_lines = payload
            .otp_lines
            .iter()
            .map(|line| {
                let tags = line
                    .tags
                    .iter()
                    .map(|tag| {
                        OtpTag::parse(otp_project, parse("otp_lines.tags", tag)?)
                            .map_err(wire("otp_lines.tags"))
                    })
                    .collect::<Result<Vec<_>, PlanError>>()?;
                Ok(OtpLine::new(
                    parse::<OtpLineVersion>("otp_lines.line", &line.line)?,
                    tags,
                )?)
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let otp_lines = NonEmpty::try_from(otp_lines).map_err(wire("otp_lines"))?;
        JigsawPlan::assemble(PlanParts {
            host: parse::<HostName>("host", &payload.host)?,
            host_project: host_project_name,
            otp_project: otp_project_name,
            releases,
            otp_lines,
            untracked_dependencies: parse_set::<DependencyName>(
                "untracked_dependencies",
                &payload.untracked_dependencies,
            )?,
            excluded_otp_apps: parse_set::<ApplicationName>(
                "excluded_otp_apps",
                &payload.excluded_otp_apps,
            )?,
            host_facts: parse::<HostFactsDigest>("host_facts_digest", &payload.host_facts_digest)?,
            tag_source: parse::<TagSourceKind>("tag_source", &payload.tag_source)?,
            extractor_version: parse::<ExtractorVersion>(
                "extractor_version",
                &payload.extractor_version,
            )?,
        })
    }
}

fn parse_set<T: FromStr + Ord>(
    field: &'static str,
    values: &[String],
) -> Result<BTreeSet<T>, PlanError>
where
    T::Err: Display,
{
    values.iter().map(|value| parse(field, value)).collect()
}
