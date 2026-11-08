// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Host;
use crate::digest::DigestWriter;
use crate::errors::{ConfigError, StoreError};
use crate::jigsaw::digest::host_facts_digest;
use crate::jigsaw::releases::{HasHostTag, SeriesReleases, SeriesReleasesError};
use crate::jigsaw::tags::{OtpLine, OtpLineError};
use crate::model::names::{
    ApplicationName, CommitSha, DependencyName, ExtractorVersion, HostFactsDigest, HostName,
    PlanCacheKey, ProjectName, TagName, vocabulary,
};
use crate::model::non_empty::NonEmpty;
use crate::model::pin::Pin;
use crate::model::release_version::{
    ErlangMajor, ErlangVersionRange, HostSeries, HostTag, OtpLineVersion,
};
use crate::model::snapshot::FORMAT_VERSION;

const CACHE_KEY_DIGEST_BYTES: usize = 8;

vocabulary!(
    pub enum TagSourceKind: "tag source" {
        Repository => "repository",
        Store => "store",
    }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRelease {
    pub tag: HostTag,
    pub commit: CommitSha,
    pub pins: BTreeMap<ProjectName, TagName>,
    pub erlang_support: ErlangVersionRange,
}

impl HasHostTag for HostRelease {
    fn host_tag(&self) -> &HostTag {
        &self.tag
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFacts {
    pub tag: HostTag,
    pub commit: CommitSha,
    pub pins: BTreeMap<ProjectName, TagName>,
}

impl HasHostTag for ReleaseFacts {
    fn host_tag(&self) -> &HostTag {
        &self.tag
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedMajor {
    pub release: TagName,
    pub range: ErlangVersionRange,
    pub line: OtpLineVersion,
}

impl fmt::Display for UnsupportedMajor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} supports Erlang {}, which excludes {} (major {})",
            self.release,
            self.range,
            self.line,
            self.line.major()
        )
    }
}

fn join_failures(failures: &[UnsupportedMajor]) -> String {
    failures
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("no release of series {series} matches the host's tag pattern")]
    EmptySeries { series: HostSeries },

    #[error("--up-to {up_to} is not a release of series {series}")]
    UpToNotInSeries { up_to: TagName, series: HostSeries },

    #[error("no OTP release tag for Erlang major {major}")]
    NoOtpTags { major: ErlangMajor },

    #[error("{}", join_failures(failures))]
    ErlangMajorUnsupported { failures: Vec<UnsupportedMajor> },

    #[error("release {tag}: {detail}")]
    ReleaseUnreadable { tag: TagName, detail: String },

    #[error("plan field {field}: {detail}")]
    Wire { field: &'static str, detail: String },

    #[error(transparent)]
    Releases(#[from] SeriesReleasesError),

    #[error(transparent)]
    OtpLine(#[from] OtpLineError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JigsawPlan {
    host: HostName,
    host_project: ProjectName,
    otp_project: ProjectName,
    releases: SeriesReleases<HostRelease>,
    otp_lines: NonEmpty<OtpLine>,
    untracked_dependencies: BTreeSet<DependencyName>,
    excluded_otp_apps: BTreeSet<ApplicationName>,
    host_facts: HostFactsDigest,
    tag_source: TagSourceKind,
    extractor_version: ExtractorVersion,
    cache_key: PlanCacheKey,
}

pub(super) struct PlanParts {
    pub(super) host: HostName,
    pub(super) host_project: ProjectName,
    pub(super) otp_project: ProjectName,
    pub(super) releases: SeriesReleases<HostRelease>,
    pub(super) otp_lines: NonEmpty<OtpLine>,
    pub(super) untracked_dependencies: BTreeSet<DependencyName>,
    pub(super) excluded_otp_apps: BTreeSet<ApplicationName>,
    pub(super) host_facts: HostFactsDigest,
    pub(super) tag_source: TagSourceKind,
    pub(super) extractor_version: ExtractorVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftInputs {
    pub releases: SeriesReleases<ReleaseFacts>,
    pub otp_lines: NonEmpty<OtpLine>,
    pub untracked_dependencies: BTreeSet<DependencyName>,
    pub excluded_otp_apps: BTreeSet<ApplicationName>,
    pub tag_source: TagSourceKind,
    pub extractor_version: ExtractorVersion,
}

pub fn draft_plan(host: &Host, inputs: DraftInputs) -> Result<JigsawPlan, PlanError> {
    let releases = inputs.releases.map(|facts| HostRelease {
        erlang_support: host.erlang_support.range_for(facts.tag.version()).clone(),
        tag: facts.tag,
        commit: facts.commit,
        pins: facts.pins,
    })?;
    JigsawPlan::assemble(PlanParts {
        host: host.name.clone(),
        host_project: host.project.clone(),
        otp_project: host.otp_project.clone(),
        releases,
        otp_lines: inputs.otp_lines,
        untracked_dependencies: inputs.untracked_dependencies,
        excluded_otp_apps: inputs.excluded_otp_apps,
        host_facts: host_facts_digest(&host.pins, &host.release_roots),
        tag_source: inputs.tag_source,
        extractor_version: inputs.extractor_version,
    })
}

impl JigsawPlan {
    pub(super) fn assemble(parts: PlanParts) -> Result<Self, PlanError> {
        let failures: Vec<UnsupportedMajor> = parts
            .releases
            .iter()
            .flat_map(|release| {
                parts
                    .otp_lines
                    .iter()
                    .filter(|line| !release.erlang_support.contains(line.line()))
                    .map(|line| UnsupportedMajor {
                        release: release.tag.tag().clone(),
                        range: release.erlang_support.clone(),
                        line: line.line(),
                    })
            })
            .collect();
        if !failures.is_empty() {
            return Err(PlanError::ErlangMajorUnsupported { failures });
        }
        let required = required_pins(
            &parts.host_project,
            &parts.releases,
            &parts.otp_project,
            &parts.otp_lines,
        );
        let cache_key = cache_key(&parts, &required);
        Ok(Self {
            host: parts.host,
            host_project: parts.host_project,
            otp_project: parts.otp_project,
            releases: parts.releases,
            otp_lines: parts.otp_lines,
            untracked_dependencies: parts.untracked_dependencies,
            excluded_otp_apps: parts.excluded_otp_apps,
            host_facts: parts.host_facts,
            tag_source: parts.tag_source,
            extractor_version: parts.extractor_version,
            cache_key,
        })
    }

    #[must_use]
    pub fn host(&self) -> &HostName {
        &self.host
    }

    #[must_use]
    pub fn host_project(&self) -> &ProjectName {
        &self.host_project
    }

    #[must_use]
    pub fn otp_project(&self) -> &ProjectName {
        &self.otp_project
    }

    #[must_use]
    pub fn series(&self) -> HostSeries {
        self.releases.series()
    }

    #[must_use]
    pub fn releases(&self) -> &SeriesReleases<HostRelease> {
        &self.releases
    }

    #[must_use]
    pub fn otp_lines(&self) -> &NonEmpty<OtpLine> {
        &self.otp_lines
    }

    #[must_use]
    pub fn untracked_dependencies(&self) -> &BTreeSet<DependencyName> {
        &self.untracked_dependencies
    }

    #[must_use]
    pub fn excluded_otp_apps(&self) -> &BTreeSet<ApplicationName> {
        &self.excluded_otp_apps
    }

    #[must_use]
    pub fn host_facts(&self) -> &HostFactsDigest {
        &self.host_facts
    }

    #[must_use]
    pub fn tag_source(&self) -> TagSourceKind {
        self.tag_source
    }

    #[must_use]
    pub fn extractor_version(&self) -> &ExtractorVersion {
        &self.extractor_version
    }

    #[must_use]
    pub fn cache_key(&self) -> &PlanCacheKey {
        &self.cache_key
    }

    #[must_use]
    pub fn erlang_majors(&self) -> BTreeSet<ErlangMajor> {
        erlang_majors(&self.otp_lines)
    }

    #[must_use]
    pub fn required(&self) -> BTreeSet<Pin> {
        required_pins(
            &self.host_project,
            &self.releases,
            &self.otp_project,
            &self.otp_lines,
        )
    }
}

fn required_pins(
    host_project: &ProjectName,
    releases: &SeriesReleases<HostRelease>,
    otp_project: &ProjectName,
    otp_lines: &NonEmpty<OtpLine>,
) -> BTreeSet<Pin> {
    let mut pins = BTreeSet::new();
    for release in releases {
        pins.insert(Pin::new(host_project.clone(), release.tag.tag().clone()));
        for (project, tag) in &release.pins {
            pins.insert(Pin::new(project.clone(), tag.clone()));
        }
    }
    for line in otp_lines {
        for tag in line.tags() {
            pins.insert(Pin::new(otp_project.clone(), tag.tag().clone()));
        }
    }
    pins
}

fn erlang_majors(otp_lines: &NonEmpty<OtpLine>) -> BTreeSet<ErlangMajor> {
    otp_lines.iter().map(|line| line.line().major()).collect()
}

fn cache_key(parts: &PlanParts, required: &BTreeSet<Pin>) -> PlanCacheKey {
    let mut writer = DigestWriter::new("jigsaw plan cache key");
    writer.count(required.len());
    for pin in required {
        writer.text(pin.project.as_str()).text(pin.tag.as_str());
    }
    let majors = erlang_majors(&parts.otp_lines);
    writer.count(majors.len());
    for major in majors {
        writer.text(&major.to_string());
    }
    writer.text(parts.host_facts.as_str());
    let digest = writer.finish();
    let mut short = [0; CACHE_KEY_DIGEST_BYTES];
    short.copy_from_slice(&digest[..CACHE_KEY_DIGEST_BYTES]);
    PlanCacheKey::compose(
        &parts.host,
        parts.releases.series(),
        FORMAT_VERSION,
        &parts.extractor_version,
        short,
    )
}
