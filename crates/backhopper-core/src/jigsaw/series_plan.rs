// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{Config, Host};
use crate::erlang_mk::{DepPin, dep_to_tag};
use crate::jigsaw::plan::{
    DraftInputs, JigsawPlan, PlanError, ReleaseFacts, TagSourceKind, draft_plan,
};
use crate::jigsaw::releases::SeriesReleases;
use crate::jigsaw::tags::{select_otp_lines, select_series_tags};
use crate::model::names::{
    ApplicationName, CommitSha, DependencyName, ExtractorVersion, ProjectName, TagName,
};
use crate::model::non_empty::NonEmpty;
use crate::model::release_version::{ErlangMajor, HostSeries, HostTag};
use crate::store::{ReadOnly, SnapshotStore};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedTags {
    pub host: Vec<TagName>,
    pub otp: Vec<TagName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesRequest {
    pub series: HostSeries,
    pub up_to: Option<TagName>,
    pub majors: NonEmpty<ErlangMajor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRelease {
    pub commit: CommitSha,
    pub pins: Vec<DepPin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftedPlan {
    pub plan: JigsawPlan,
    pub skipped_tags: Vec<TagName>,
}

/// A dependency with no tag-based project is untracked.
fn release_facts(
    cfg: &Config,
    host: &Host,
    tag: HostTag,
    resolved: ResolvedRelease,
    untracked: &mut BTreeSet<DependencyName>,
) -> Result<ReleaseFacts, PlanError> {
    let mut pins: BTreeMap<ProjectName, TagName> = BTreeMap::new();
    for pin in &resolved.pins {
        let project = ProjectName::new(pin.name.as_str())
            .ok()
            .filter(|name| *name != host.project && *name != host.otp_project)
            .and_then(|name| cfg.project(&name).ok());
        let Some(project) = project.filter(|project| !project.is_self()) else {
            untracked.insert(pin.name.clone());
            continue;
        };
        let raw = dep_to_tag(pin, &project.tag_prefix);
        let dep_tag = TagName::new(raw.clone()).map_err(|e| PlanError::ReleaseUnreadable {
            tag: tag.tag().clone(),
            detail: format!("pin {} resolves to {raw:?}: {e}", pin.name),
        })?;
        pins.insert(project.name.clone(), dep_tag);
    }
    Ok(ReleaseFacts {
        tag,
        commit: resolved.commit,
        pins,
    })
}

/// `resolve` is the only part that differs between a repository and the store.
pub fn plan_series(
    cfg: &Config,
    host: &Host,
    request: &SeriesRequest,
    listed: &ListedTags,
    tag_source: TagSourceKind,
    extractor_version: ExtractorVersion,
    mut resolve: impl FnMut(&HostTag) -> Result<ResolvedRelease, PlanError>,
) -> Result<DraftedPlan, PlanError> {
    let host_project = cfg.project(&host.project)?;
    let otp_project = cfg.project(&host.otp_project)?;
    let selected = select_series_tags(
        host_project,
        &host.series_tag_pattern,
        request.series,
        request.up_to.as_ref(),
        &listed.host,
    )?;
    let otp_lines = select_otp_lines(otp_project, &request.majors, &listed.otp)?;
    let mut untracked_dependencies = BTreeSet::new();
    let facts = selected
        .releases
        .iter()
        .map(|tag| {
            let resolved = resolve(tag)?;
            release_facts(
                cfg,
                host,
                tag.clone(),
                resolved,
                &mut untracked_dependencies,
            )
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let plan = draft_plan(
        host,
        DraftInputs {
            releases: SeriesReleases::new(facts)?,
            otp_lines,
            untracked_dependencies,
            excluded_otp_apps: otp_project
                .exclude_apps
                .iter()
                .cloned()
                .collect::<BTreeSet<ApplicationName>>(),
            tag_source,
            extractor_version,
        },
    )?;
    Ok(DraftedPlan {
        plan,
        skipped_tags: selected.skipped_tags,
    })
}

/// A release with no host snapshot yet is not in the plan.
pub fn plan_series_from_store(
    cfg: &Config,
    host: &Host,
    request: &SeriesRequest,
    store: &SnapshotStore<ReadOnly>,
    extractor_version: ExtractorVersion,
) -> Result<DraftedPlan, PlanError> {
    let listed = ListedTags {
        host: store.list_tags(&host.project)?,
        otp: store.list_tags(&host.otp_project)?,
    };
    plan_series(
        cfg,
        host,
        request,
        &listed,
        TagSourceKind::Store,
        extractor_version,
        |tag| {
            let header = store.read_header(&host.project, tag.tag()).map_err(|e| {
                PlanError::ReleaseUnreadable {
                    tag: tag.tag().clone(),
                    detail: e.to_string(),
                }
            })?;
            Ok(ResolvedRelease {
                commit: header.commit,
                pins: header.dep_pins,
            })
        },
    )
}
