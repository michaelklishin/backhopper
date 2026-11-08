// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::jigsaw::findings::{Finding, Stop, Warning, WithheldReason, WithheldSymbol};
use crate::jigsaw::plan::{JigsawPlan, TagSourceKind};
use crate::jigsaw::plugin_set::AppRole;
use crate::jigsaw::references::{NotChecked, ReferenceSite, UncheckedSite};
use crate::model::names::{ApplicationName, HostName, StoreDigest, vocabulary};
use crate::model::non_empty::NonEmpty;
use crate::model::release_version::{HostSeries, HostTag, OtpTag};

vocabulary!(
    /// The state of one app at one target.
    #[derive(PartialOrd, Ord, Hash)]
    pub enum JigsawCell: "cell state" {
        Clean => "clean",
        Broken => "broken",
    }
);

mod private {
    pub trait Sealed {}
}

/// An index into one axis of `TargetAxes`: the only two are `HostIdx`
/// and `OtpIdx`.
pub trait AxisIndex: private::Sealed + Copy + Ord {
    fn position(self) -> usize;
    fn adjacent_to(self, earlier: Self) -> bool {
        self.position() == earlier.position() + 1
    }
}

/// A host release, by position in the plan's ordered releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostIdx(usize);

/// An OTP tag, by position in the plan's ordered OTP tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OtpIdx(usize);

impl private::Sealed for HostIdx {}
impl private::Sealed for OtpIdx {}

impl AxisIndex for HostIdx {
    fn position(self) -> usize {
        self.0
    }
}

impl AxisIndex for OtpIdx {
    fn position(self) -> usize {
        self.0
    }
}

/// A maximal stretch of adjacent members, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run<I> {
    pub first: I,
    pub last: I,
}

/// The two ordered axes of a run, and the only source of `HostIdx` and
/// `OtpIdx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetAxes {
    hosts: Vec<HostTag>,
    otp: Vec<OtpTag>,
    build: HostIdx,
}

impl TargetAxes {
    #[must_use]
    pub(crate) fn of(plan: &JigsawPlan) -> Self {
        let hosts: Vec<HostTag> = plan
            .releases()
            .iter()
            .map(|release| release.tag.clone())
            .collect();
        let otp = plan
            .otp_lines()
            .iter()
            .flat_map(|line| line.tags().iter().cloned())
            .collect();
        let build = HostIdx(hosts.len() - 1);
        Self { hosts, otp, build }
    }

    #[must_use]
    pub fn hosts(&self) -> &[HostTag] {
        &self.hosts
    }

    #[must_use]
    pub fn otp_tags(&self) -> &[OtpTag] {
        &self.otp
    }

    #[must_use]
    pub fn build(&self) -> HostIdx {
        self.build
    }

    #[must_use]
    pub fn host_tag(&self, index: HostIdx) -> &HostTag {
        &self.hosts[index.0]
    }

    #[must_use]
    pub fn otp_tag(&self, index: OtpIdx) -> &OtpTag {
        &self.otp[index.0]
    }

    pub fn host_indices(&self) -> impl Iterator<Item = HostIdx> + use<> {
        (0..self.hosts.len()).map(HostIdx)
    }

    pub fn otp_indices(&self) -> impl Iterator<Item = OtpIdx> + use<> {
        (0..self.otp.len()).map(OtpIdx)
    }

    /// The runs of adjacent members of `members`, ascending. Runs are
    /// sorted, disjoint and never adjacent to each other, because they are
    /// derived from a set.
    #[must_use]
    pub fn runs<I: AxisIndex>(members: &BTreeSet<I>) -> Vec<Run<I>> {
        let mut runs: Vec<Run<I>> = Vec::new();
        for &index in members {
            match runs.last_mut() {
                Some(run) if index.adjacent_to(run.last) => run.last = index,
                _ => runs.push(Run {
                    first: index,
                    last: index,
                }),
            }
        }
        runs
    }
}

/// Which targets a finding or warning covers. The axis is part of the
/// type: a host finding covers every OTP tag, an OTP finding covers every
/// host release, and an app nothing provides covers everything. A finding
/// on some of both axes cannot be written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingSpan {
    Host(BTreeSet<HostIdx>),
    Otp(BTreeSet<OtpIdx>),
    Everywhere,
}

impl FindingSpan {
    #[must_use]
    pub fn covers(&self, host: HostIdx, otp: OtpIdx) -> bool {
        match self {
            Self::Host(releases) => releases.contains(&host),
            Self::Otp(tags) => tags.contains(&otp),
            Self::Everywhere => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangedFinding {
    pub finding: Finding,
    pub apps: NonEmpty<ApplicationName>,
    pub sites: NonEmpty<ReferenceSite>,
    pub span: FindingSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangedWithheld {
    pub symbol: WithheldSymbol,
    pub sites: NonEmpty<ReferenceSite>,
    pub span: FindingSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangedWarning {
    pub warning: Warning,
    pub span: FindingSpan,
}

/// What the run covered: every distinct reference compared at every
/// target, and what it did not compare, by reason. Every label of both
/// vocabularies is present, with zero where nothing was counted. The
/// count of a listed reason is the number of unchecked or withheld sites
/// with it. The other reasons count anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tallies {
    checked: usize,
    not_checked: BTreeMap<NotChecked, usize>,
    withheld: BTreeMap<WithheldReason, usize>,
}

impl Tallies {
    #[must_use]
    pub(crate) fn new(
        checked: usize,
        not_checked: impl IntoIterator<Item = (NotChecked, usize)>,
        unchecked: &[UncheckedSite],
        stopped: impl IntoIterator<Item = (Stop, usize)>,
        withheld: &[RangedWithheld],
    ) -> Self {
        let mut tallies = Self {
            checked,
            not_checked: NotChecked::ALL.iter().map(|&r| (r, 0)).collect(),
            withheld: WithheldReason::ALL.iter().map(|&r| (r, 0)).collect(),
        };
        for (reason, n) in not_checked {
            *tallies.not_checked.entry(reason).or_default() += n;
        }
        for site in unchecked {
            *tallies.not_checked.entry(site.reason()).or_default() += 1;
        }
        for (stop, n) in stopped {
            *tallies.withheld.entry(stop.reason()).or_default() += n;
        }
        for ranged in withheld {
            *tallies.withheld.entry(ranged.symbol.reason()).or_default() +=
                ranged.sites.as_slice().len();
        }
        tallies
    }

    #[must_use]
    pub fn checked(&self) -> usize {
        self.checked
    }

    #[must_use]
    pub fn not_checked(&self) -> &BTreeMap<NotChecked, usize> {
        &self.not_checked
    }

    #[must_use]
    pub fn withheld(&self) -> &BTreeMap<WithheldReason, usize> {
        &self.withheld
    }
}

pub(crate) struct ReportParts {
    pub host: HostName,
    pub series: HostSeries,
    pub tag_source: TagSourceKind,
    pub targets: TargetAxes,
    pub apps: BTreeMap<ApplicationName, AppRole>,
    pub findings: Vec<RangedFinding>,
    pub warnings: Vec<RangedWarning>,
    pub withheld: Vec<RangedWithheld>,
    pub tallies: Tallies,
    pub unchecked: Vec<UncheckedSite>,
    pub host_provided: BTreeSet<ApplicationName>,
    pub store_digest: StoreDigest,
}

/// The answer of a run. The matrix is not stored: `cell` and `is_clean`
/// read the findings, so there is no second copy to disagree with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JigsawReport {
    host: HostName,
    series: HostSeries,
    tag_source: TagSourceKind,
    targets: TargetAxes,
    apps: BTreeMap<ApplicationName, AppRole>,
    findings: Vec<RangedFinding>,
    warnings: Vec<RangedWarning>,
    withheld: Vec<RangedWithheld>,
    tallies: Tallies,
    unchecked: Vec<UncheckedSite>,
    host_provided: BTreeSet<ApplicationName>,
    store_digest: StoreDigest,
}

impl JigsawReport {
    pub(crate) fn from_parts(parts: ReportParts) -> Self {
        Self {
            host: parts.host,
            series: parts.series,
            tag_source: parts.tag_source,
            targets: parts.targets,
            apps: parts.apps,
            findings: parts.findings,
            warnings: parts.warnings,
            withheld: parts.withheld,
            tallies: parts.tallies,
            unchecked: parts.unchecked,
            host_provided: parts.host_provided,
            store_digest: parts.store_digest,
        }
    }

    #[must_use]
    pub fn host(&self) -> &HostName {
        &self.host
    }

    #[must_use]
    pub fn series(&self) -> HostSeries {
        self.series
    }

    #[must_use]
    pub fn tag_source(&self) -> TagSourceKind {
        self.tag_source
    }

    #[must_use]
    pub fn targets(&self) -> &TargetAxes {
        &self.targets
    }

    #[must_use]
    pub fn build_release(&self) -> &HostTag {
        self.targets.host_tag(self.targets.build())
    }

    #[must_use]
    pub fn apps(&self) -> &BTreeMap<ApplicationName, AppRole> {
        &self.apps
    }

    #[must_use]
    pub fn findings(&self) -> &[RangedFinding] {
        &self.findings
    }

    #[must_use]
    pub fn warnings(&self) -> &[RangedWarning] {
        &self.warnings
    }

    /// The comparisons that were not made, ordered by axis and symbol.
    #[must_use]
    pub fn withheld(&self) -> &[RangedWithheld] {
        &self.withheld
    }

    #[must_use]
    pub fn tallies(&self) -> &Tallies {
        &self.tallies
    }

    /// The places behind the listed reasons of the tallies, ordered by
    /// reason, file, line and symbol.
    #[must_use]
    pub fn unchecked(&self) -> &[UncheckedSite] {
        &self.unchecked
    }

    #[must_use]
    pub fn host_provided(&self) -> &BTreeSet<ApplicationName> {
        &self.host_provided
    }

    #[must_use]
    pub fn store_digest(&self) -> &StoreDigest {
        &self.store_digest
    }

    /// An app is broken at a target when a finding that names it covers
    /// the target.
    #[must_use]
    pub fn cell(&self, app: &ApplicationName, host: HostIdx, otp: OtpIdx) -> JigsawCell {
        let broken = self.findings.iter().any(|ranged| {
            ranged.apps.iter().any(|named| named == app) && ranged.span.covers(host, otp)
        });
        if broken {
            JigsawCell::Broken
        } else {
            JigsawCell::Clean
        }
    }

    /// Warnings never count.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}
