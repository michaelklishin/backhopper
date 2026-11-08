// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::jigsaw::findings::{JigsawFindingKind, JigsawWarningKind, WithheldReason};
use crate::jigsaw::plan::TagSourceKind;
use crate::jigsaw::plugin_set::AppRole;
use crate::jigsaw::references::{NotChecked, ReferenceSite};
use crate::jigsaw::report::{FindingSpan, JigsawCell, JigsawReport, TargetAxes};
use crate::model::names::{ApplicationName, RelativePath};
use crate::model::non_empty::NonEmpty;

/// What `jigsaw check` does not compare, in every report.
pub const LIMITS: [&str; 3] = [
    "OTP header records and macros are not compared",
    "calls inside OTP header macros are not checked",
    "dependencies are checked at the exact versions each host release pins, not over a range",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawCheckPayload {
    pub host: String,
    pub series: String,
    pub tag_source: TagSourceKind,
    pub build_release: String,
    pub host_releases: Vec<String>,
    pub otp_tags: Vec<String>,
    pub apps: Vec<JigsawAppRowPayload>,
    pub findings: Vec<JigsawFindingPayload>,
    pub warnings: Vec<JigsawWarningPayload>,
    pub host_provided: Vec<ApplicationName>,
    pub tallies: JigsawTalliesPayload,
    pub unchecked: Vec<JigsawUncheckedPayload>,
    pub withheld: Vec<JigsawWithheldPayload>,
    pub limits: Vec<String>,
    pub store_digest: String,
    pub clean: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawAppRowPayload {
    pub app: ApplicationName,
    pub role: AppRole,
    pub cells: Vec<JigsawCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawFindingPayload {
    pub kind: JigsawFindingKind,
    pub symbol: String,
    pub detail: Option<String>,
    pub apps: Vec<ApplicationName>,
    pub sites: Vec<JigsawSitePayload>,
    pub host_release_ranges: Vec<JigsawRangePayload>,
    pub otp_tag_ranges: Vec<JigsawRangePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawWarningPayload {
    pub kind: JigsawWarningKind,
    pub symbol: String,
    pub detail: Option<String>,
    pub host_release_ranges: Vec<JigsawRangePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawSitePayload {
    pub app: ApplicationName,
    pub path: RelativePath,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawUncheckedPayload {
    pub reason: NotChecked,
    pub symbol: String,
    pub app: ApplicationName,
    pub path: RelativePath,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawWithheldPayload {
    pub reason: WithheldReason,
    pub symbol: String,
    pub sites: Vec<JigsawSitePayload>,
    pub host_release_ranges: Vec<JigsawRangePayload>,
    pub otp_tag_ranges: Vec<JigsawRangePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawRangePayload {
    pub first: String,
    pub last: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JigsawTalliesPayload {
    pub checked: u64,
    pub not_checked: BTreeMap<NotChecked, u64>,
    pub withheld: BTreeMap<WithheldReason, u64>,
}

fn ranges(
    report: &JigsawReport,
    span: &FindingSpan,
) -> (Vec<JigsawRangePayload>, Vec<JigsawRangePayload>) {
    let targets = report.targets();
    match span {
        FindingSpan::Host(members) => (
            TargetAxes::runs(members)
                .into_iter()
                .map(|run| JigsawRangePayload {
                    first: targets.host_tag(run.first).tag().to_string(),
                    last: targets.host_tag(run.last).tag().to_string(),
                })
                .collect(),
            Vec::new(),
        ),
        FindingSpan::Otp(members) => (
            Vec::new(),
            TargetAxes::runs(members)
                .into_iter()
                .map(|run| JigsawRangePayload {
                    first: targets.otp_tag(run.first).tag().to_string(),
                    last: targets.otp_tag(run.last).tag().to_string(),
                })
                .collect(),
        ),
        FindingSpan::Everywhere => (Vec::new(), Vec::new()),
    }
}

fn sites(sites: &NonEmpty<ReferenceSite>) -> Vec<JigsawSitePayload> {
    sites
        .iter()
        .map(|site| JigsawSitePayload {
            app: site.file.app.clone(),
            path: site.file.path.clone(),
            line: site.line,
        })
        .collect()
}

fn counts<K: Ord + Copy>(tally: &BTreeMap<K, usize>) -> BTreeMap<K, u64> {
    tally.iter().map(|(&key, &n)| (key, n as u64)).collect()
}

impl From<&JigsawReport> for JigsawCheckPayload {
    fn from(report: &JigsawReport) -> Self {
        let targets = report.targets();
        let tallies = report.tallies();
        Self {
            host: report.host().to_string(),
            series: report.series().to_string(),
            tag_source: report.tag_source(),
            build_release: report.build_release().tag().to_string(),
            host_releases: targets
                .hosts()
                .iter()
                .map(|tag| tag.tag().to_string())
                .collect(),
            otp_tags: targets
                .otp_tags()
                .iter()
                .map(|tag| tag.tag().to_string())
                .collect(),
            apps: report
                .apps()
                .iter()
                .map(|(app, &role)| JigsawAppRowPayload {
                    app: app.clone(),
                    role,
                    cells: targets
                        .host_indices()
                        .flat_map(|host| {
                            targets
                                .otp_indices()
                                .map(move |otp| report.cell(app, host, otp))
                        })
                        .collect(),
                })
                .collect(),
            findings: report
                .findings()
                .iter()
                .map(|ranged| {
                    let (host_release_ranges, otp_tag_ranges) = ranges(report, &ranged.span);
                    JigsawFindingPayload {
                        kind: ranged.finding.kind(),
                        symbol: ranged.finding.symbol(),
                        detail: ranged.finding.detail(),
                        apps: ranged.apps.iter().cloned().collect(),
                        sites: sites(&ranged.sites),
                        host_release_ranges,
                        otp_tag_ranges,
                    }
                })
                .collect(),
            warnings: report
                .warnings()
                .iter()
                .map(|ranged| JigsawWarningPayload {
                    kind: ranged.warning.kind(),
                    symbol: ranged.warning.symbol(),
                    detail: ranged.warning.detail(),
                    host_release_ranges: ranges(report, &ranged.span).0,
                })
                .collect(),
            host_provided: report.host_provided().iter().cloned().collect(),
            tallies: JigsawTalliesPayload {
                checked: tallies.checked() as u64,
                not_checked: counts(tallies.not_checked()),
                withheld: counts(tallies.withheld()),
            },
            unchecked: report
                .unchecked()
                .iter()
                .map(|site| JigsawUncheckedPayload {
                    reason: site.reason(),
                    symbol: site.symbol.to_string(),
                    app: site.file.app.clone(),
                    path: site.file.path.clone(),
                    line: site.line,
                })
                .collect(),
            withheld: report
                .withheld()
                .iter()
                .map(|ranged| {
                    let (host_release_ranges, otp_tag_ranges) = ranges(report, &ranged.span);
                    JigsawWithheldPayload {
                        reason: ranged.symbol.reason(),
                        symbol: ranged.symbol.to_string(),
                        sites: sites(&ranged.sites),
                        host_release_ranges,
                        otp_tag_ranges,
                    }
                })
                .collect(),
            limits: LIMITS.iter().map(|&limit| limit.to_owned()).collect(),
            store_digest: report.store_digest().to_string(),
            clean: report.is_clean(),
        }
    }
}
