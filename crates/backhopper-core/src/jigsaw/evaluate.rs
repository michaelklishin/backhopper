// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};

use crate::jigsaw::anchor::{Anchored, anchor};
use crate::jigsaw::check::{check_host, check_otp};
use crate::jigsaw::findings::{Checked, Finding, Stop, Warning, Withheld, WithheldSymbol};
use crate::jigsaw::loaded::LoadedPlan;
use crate::jigsaw::plugin_set::PluginSet;
use crate::jigsaw::references::{ReferenceSet, ReferenceSite, UncheckedSite};
use crate::jigsaw::report::{
    FindingSpan, HostIdx, JigsawReport, OtpIdx, RangedFinding, RangedWarning, RangedWithheld,
    ReportParts, Tallies, TargetAxes,
};
use crate::jigsaw::surface::{AppPresence, Blocker, ReleaseSurface};
use crate::model::names::ApplicationName;
use crate::model::non_empty::NonEmpty;

#[derive(Debug, Clone, Copy)]
enum Target {
    Host(HostIdx),
    Otp(OtpIdx),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Axis {
    Host,
    Otp,
    Everywhere,
}

impl Target {
    fn axis(self) -> Axis {
        match self {
            Self::Host(_) => Axis::Host,
            Self::Otp(_) => Axis::Otp,
        }
    }
}

#[derive(Default)]
struct Hit {
    hosts: BTreeSet<HostIdx>,
    otp: BTreeSet<OtpIdx>,
    sites: BTreeSet<ReferenceSite>,
}

impl Hit {
    fn add(&mut self, target: Target, sites: &BTreeSet<ReferenceSite>) {
        match target {
            Target::Host(host) => self.hosts.insert(host),
            Target::Otp(otp) => self.otp.insert(otp),
        };
        self.sites.extend(sites.iter().cloned());
    }

    fn span(self, axis: Axis) -> (FindingSpan, BTreeSet<ReferenceSite>) {
        let span = match axis {
            Axis::Host => FindingSpan::Host(self.hosts),
            Axis::Otp => FindingSpan::Otp(self.otp),
            Axis::Everywhere => FindingSpan::Everywhere,
        };
        (span, self.sites)
    }
}

/// The results of the per-surface passes, before they become a report.
#[derive(Default)]
struct Accumulator {
    findings: BTreeMap<(Axis, Finding), Hit>,
    warnings: BTreeMap<Warning, BTreeSet<HostIdx>>,
    withheld: BTreeMap<(Axis, WithheldSymbol), Hit>,
    stopped: BTreeMap<Stop, BTreeSet<(Axis, usize)>>,
}

impl Accumulator {
    fn record(
        &mut self,
        target: Target,
        anchor_index: usize,
        sites: &BTreeSet<ReferenceSite>,
        checked: Checked,
    ) {
        for finding in checked.findings {
            self.findings
                .entry((target.axis(), finding))
                .or_default()
                .add(target, sites);
        }
        if let Target::Host(host) = target {
            for warning in checked.warnings {
                self.warnings.entry(warning).or_default().insert(host);
            }
        }
        for withheld in checked.withheld {
            match withheld {
                Withheld::Symbol(symbol) => self
                    .withheld
                    .entry((target.axis(), symbol))
                    .or_default()
                    .add(target, sites),
                Withheld::Stopped(stop) => {
                    self.stopped
                        .entry(stop)
                        .or_default()
                        .insert((target.axis(), anchor_index));
                }
            }
        }
    }
}

/// Runs the plugin set against every target of a loaded plan.
///
/// The surfaces are borrowed views built here from the loaded snapshots.
/// `collect` reads the plugin set against the build patch's surface and
/// returns its references, so a reference set cannot come from a different
/// build patch. Host anchors are checked once per host release and OTP
/// anchors once per OTP tag: `N + M` passes give the `N x M` cells.
pub fn evaluate<E>(
    loaded: &LoadedPlan,
    plugins: &PluginSet,
    collect: impl FnOnce(&PluginSet, &ReleaseSurface<'_>) -> Result<ReferenceSet, E>,
) -> Result<JigsawReport, E> {
    let surfaces = loaded.surfaces();
    let references = collect(plugins, surfaces.build())?;
    let anchored = anchor(plugins, &references, surfaces.build(), surfaces.otp());
    let axes = TargetAxes::of(loaded.plan());
    let mut acc = Accumulator::default();

    for (host, release) in axes.host_indices().zip(surfaces.releases()) {
        for (index, anchor) in anchored.host.iter().enumerate() {
            let checked = check_host(&anchor.kind, release);
            acc.record(Target::Host(host), index, &anchor.sites, checked);
        }
        for (app, blocker) in release.shipped().blockers() {
            let warning = match blocker {
                Blocker::Manifest(reason) => Warning::ManifestUnreadable {
                    app: app.clone(),
                    reason: reason.clone(),
                },
                Blocker::Untracked => Warning::AppNotTracked { app: app.clone() },
            };
            acc.warnings.entry(warning).or_default().insert(host);
        }
        for app in plugins.apps().keys() {
            if release.shipped().lookup(app) == AppPresence::Provided {
                acc.warnings
                    .entry(Warning::BundledAppShippedByHost { app: app.clone() })
                    .or_default()
                    .insert(host);
            }
        }
    }
    for (otp, surface) in axes.otp_indices().zip(surfaces.otp()) {
        for (index, anchor) in anchored.otp.iter().enumerate() {
            let checked = check_otp(&anchor.kind, surface);
            acc.record(Target::Otp(otp), index, &anchor.sites, checked);
        }
    }
    for unprovided in &anchored.unprovided {
        let finding = Finding::AppMissing {
            app: unprovided.kind.app.clone(),
            required_by: unprovided.kind.required_by.clone(),
        };
        acc.findings
            .entry((Axis::Everywhere, finding))
            .or_default()
            .sites
            .extend(unprovided.sites.iter().cloned());
    }

    Ok(assemble(loaded, plugins, &references, &anchored, axes, acc))
}

fn assemble(
    loaded: &LoadedPlan,
    plugins: &PluginSet,
    references: &ReferenceSet,
    anchored: &Anchored,
    targets: TargetAxes,
    acc: Accumulator,
) -> JigsawReport {
    let findings = acc
        .findings
        .into_iter()
        .filter_map(|((axis, finding), hit)| {
            let (span, sites) = hit.span(axis);
            let apps: BTreeSet<ApplicationName> =
                sites.iter().map(|site| site.file.app.clone()).collect();
            Some(RangedFinding {
                finding,
                apps: NonEmpty::try_from(apps.into_iter().collect::<Vec<_>>()).ok()?,
                sites: NonEmpty::try_from(sites.into_iter().collect::<Vec<_>>()).ok()?,
                span,
            })
        })
        .collect();
    let warnings = acc
        .warnings
        .into_iter()
        .map(|(warning, hosts)| RangedWarning {
            warning,
            span: FindingSpan::Host(hosts),
        })
        .collect();
    let not_checked = references
        .not_checked()
        .iter()
        .chain(&anchored.not_checked)
        .map(|(&reason, &n)| (reason, n));
    let unchecked: Vec<UncheckedSite> = references
        .unchecked()
        .iter()
        .chain(&anchored.unchecked)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let stopped = acc
        .stopped
        .into_iter()
        .map(|(stop, anchors)| (stop, anchors.len()));
    let withheld: Vec<RangedWithheld> = acc
        .withheld
        .into_iter()
        .filter_map(|((axis, symbol), hit)| {
            let (span, sites) = hit.span(axis);
            Some(RangedWithheld {
                symbol,
                sites: NonEmpty::try_from(sites.into_iter().collect::<Vec<_>>()).ok()?,
                span,
            })
        })
        .collect();
    let plan = loaded.plan();
    JigsawReport::from_parts(ReportParts {
        host: plan.host().clone(),
        series: plan.series(),
        tag_source: plan.tag_source(),
        targets,
        apps: plugins
            .apps()
            .iter()
            .map(|(name, app)| (name.clone(), app.role()))
            .collect(),
        findings,
        warnings,
        tallies: Tallies::new(
            anchored.checked(),
            not_checked,
            &unchecked,
            stopped,
            &withheld,
        ),
        withheld,
        unchecked,
        host_provided: plugins.host_provided().clone(),
        store_digest: loaded.store_digest().clone(),
    })
}
