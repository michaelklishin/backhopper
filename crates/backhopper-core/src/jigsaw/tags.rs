// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::config::Project;
use crate::jigsaw::plan::PlanError;
use crate::jigsaw::releases::SeriesReleases;
use crate::model::names::{SeriesTagPattern, TagName};
use crate::model::non_empty::NonEmpty;
use crate::model::release_version::{ErlangMajor, HostSeries, HostTag, OtpLineVersion, OtpTag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesTags {
    pub releases: SeriesReleases<HostTag>,
    pub skipped_tags: Vec<TagName>,
}

pub fn select_series_tags(
    project: &Project,
    pattern: &SeriesTagPattern,
    series: HostSeries,
    up_to: Option<&TagName>,
    tags: &[TagName],
) -> Result<SeriesTags, PlanError> {
    let glob = pattern.for_series(series);
    let mut skipped_tags = Vec::new();
    let mut in_series = Vec::new();
    for tag in tags {
        if !glob.matches(tag) || project.is_prerelease_tag(tag) {
            continue;
        }
        match HostTag::parse(project, tag.clone()) {
            Ok(parsed) if parsed.version().series() == series => in_series.push(parsed),
            Ok(_) => {}
            Err(_) => skipped_tags.push(tag.clone()),
        }
    }
    if in_series.is_empty() {
        return Err(PlanError::EmptySeries { series });
    }
    let enumerated = SeriesReleases::new(in_series)?;
    let releases = match up_to {
        None => enumerated,
        Some(up_to) => {
            let cutoff = enumerated
                .iter()
                .find(|release| release.tag() == up_to)
                .map(|release| release.version().clone())
                .ok_or_else(|| PlanError::UpToNotInSeries {
                    up_to: up_to.clone(),
                    series,
                })?;
            let kept = enumerated
                .iter()
                .filter(|release| *release.version() <= cutoff)
                .cloned()
                .collect();
            SeriesReleases::new(kept)?
        }
    };
    Ok(SeriesTags {
        releases,
        skipped_tags,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OtpLineError {
    #[error("an OTP line needs at least one tag")]
    Empty,

    #[error("tag {tag} is not on line {line}")]
    StrayTag { tag: TagName, line: OtpLineVersion },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpLine {
    line: OtpLineVersion,
    tags: NonEmpty<OtpTag>,
}

impl OtpLine {
    pub fn new(line: OtpLineVersion, mut tags: Vec<OtpTag>) -> Result<Self, OtpLineError> {
        if let Some(stray) = tags.iter().find(|tag| tag.version().line() != line) {
            return Err(OtpLineError::StrayTag {
                tag: stray.tag().clone(),
                line,
            });
        }
        tags.sort();
        tags.dedup();
        let tags = NonEmpty::try_from(tags).map_err(|_| OtpLineError::Empty)?;
        Ok(Self { line, tags })
    }

    #[must_use]
    pub fn line(&self) -> OtpLineVersion {
        self.line
    }

    #[must_use]
    pub fn tags(&self) -> &NonEmpty<OtpTag> {
        &self.tags
    }
}

/// For each requested major, the tags of its newest minor line. Tags that
/// are not release versions (`OTP_R16B03`, `OTP-17.0-rc1`) are dropped.
pub fn select_otp_lines(
    project: &Project,
    majors: &NonEmpty<ErlangMajor>,
    tags: &[TagName],
) -> Result<NonEmpty<OtpLine>, PlanError> {
    let mut by_line: BTreeMap<OtpLineVersion, Vec<OtpTag>> = BTreeMap::new();
    for tag in tags {
        if let Ok(parsed) = OtpTag::parse(project, tag.clone()) {
            by_line
                .entry(parsed.version().line())
                .or_default()
                .push(parsed);
        }
    }
    let line_for = |major: ErlangMajor| -> Result<OtpLine, PlanError> {
        let (line, line_tags) = by_line
            .iter()
            .rev()
            .find(|(line, _)| line.major() == major)
            .ok_or(PlanError::NoOtpTags { major })?;
        Ok(OtpLine::new(*line, line_tags.clone())?)
    };
    let first = line_for(*majors.first())?;
    let rest = majors
        .iter()
        .skip(1)
        .map(|major| line_for(*major))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NonEmpty::new(first, rest))
}
