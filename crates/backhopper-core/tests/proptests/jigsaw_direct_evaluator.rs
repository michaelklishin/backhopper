// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The naive evaluator the proptests compare `evaluate` with: every
//! reference is anchored and checked at every target in turn, with nothing
//! shared between targets.

use std::collections::BTreeSet;

use backhopper_core::jigsaw::LoadedPlan;
use backhopper_core::jigsaw::anchor::anchor;
use backhopper_core::jigsaw::check::{check_host, check_otp};
use backhopper_core::jigsaw::findings::Finding;
use backhopper_core::jigsaw::plugin_set::PluginSet;
use backhopper_core::jigsaw::references::ReferenceSet;
use backhopper_core::jigsaw::report::JigsawReport;

/// A finding at the target `(host release position, OTP tag position)`.
pub(crate) type Cell = (Finding, usize, usize);

pub(crate) fn direct(
    loaded: &LoadedPlan,
    plugins: &PluginSet,
    references: &ReferenceSet,
) -> BTreeSet<Cell> {
    let surfaces = loaded.surfaces();
    let mut out = BTreeSet::new();
    for (host, release) in surfaces.releases().iter().enumerate() {
        for (otp, tag) in surfaces.otp().iter().enumerate() {
            let anchored = anchor(plugins, references, surfaces.build(), surfaces.otp());
            for a in &anchored.host {
                for finding in check_host(&a.kind, release).findings {
                    out.insert((finding, host, otp));
                }
            }
            for a in &anchored.otp {
                for finding in check_otp(&a.kind, tag).findings {
                    out.insert((finding, host, otp));
                }
            }
            for a in &anchored.unprovided {
                out.insert((
                    Finding::AppMissing {
                        app: a.kind.app.clone(),
                        required_by: a.kind.required_by.clone(),
                    },
                    host,
                    otp,
                ));
            }
        }
    }
    out
}

/// The report's findings, expanded to the targets their spans cover.
pub(crate) fn expanded(report: &JigsawReport) -> BTreeSet<Cell> {
    let hosts: Vec<_> = report.targets().host_indices().collect();
    let otps: Vec<_> = report.targets().otp_indices().collect();
    let mut out = BTreeSet::new();
    for ranged in report.findings() {
        for (host_position, host) in hosts.iter().enumerate() {
            for (otp_position, otp) in otps.iter().enumerate() {
                if ranged.span.covers(*host, *otp) {
                    out.insert((ranged.finding.clone(), host_position, otp_position));
                }
            }
        }
    }
    out
}
