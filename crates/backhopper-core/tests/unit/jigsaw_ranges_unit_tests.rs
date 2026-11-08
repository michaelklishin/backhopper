// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::path::PathBuf;

use backhopper_core::jigsaw::evaluate;
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginFile, PluginSet, PluginSources};
use backhopper_core::jigsaw::references::{Reference, ReferenceSet, ReferenceSite, SiteDetail};
use backhopper_core::jigsaw::report::{FindingSpan, HostIdx, JigsawCell, JigsawReport, TargetAxes};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::names::{ApplicationName, RelativePath};
use backhopper_test_support::{SnapshotBuilder, loaded_plan, module_with};

const RELEASES: [&str; 6] = ["v4.2.0", "v4.2.1", "v4.2.2", "v4.2.3", "v4.2.4", "v4.2.5"];

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn plugin(name: &str) -> PluginSet {
    PluginSet::new(PluginSources {
        apps: vec![PluginAppSources {
            root: PathBuf::from(format!("/plugins/{name}")),
            manifest: AppManifest {
                name: app(name),
                source: ManifestSource::AppSrc,
                requires: ManifestDeps::Listed(BTreeSet::new()),
            }
            .into_read()
            .unwrap(),
            manifest_path: RelativePath::new(format!("src/{name}.app.src")).unwrap(),
            files: Vec::new(),
        }],
        host_provided: BTreeSet::new(),
    })
    .unwrap()
}

fn site(app_name: &str) -> ReferenceSite {
    ReferenceSite {
        file: PluginFile {
            app: app(app_name),
            path: RelativePath::new("src/worker.erl").unwrap(),
        },
        line: 3,
        detail: SiteDetail::Plain,
    }
}

/// `lookup/1` is missing at releases 0, 1, 2 and 4 of six. The build patch
/// is the last release, so it is never among them.
fn report() -> JigsawReport {
    let host: Vec<(&str, SnapshotBuilder)> = RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let exports: &[(&str, u8)] = if matches!(index, 3 | 5) {
                &[("lookup", 1)]
            } else {
                &[("lookup", 2)]
            };
            let content = SnapshotBuilder::new("rabbitmq-server", tag)
                .roots(&["rabbit"])
                .app("rabbit", &[])
                .module("rabbit", module_with("rabbit_amqqueue", exports));
            (*tag, content)
        })
        .collect();
    let otp = [(
        "OTP-27.3",
        SnapshotBuilder::new("otp", "OTP-27.3").app("kernel", &[]),
    )];
    let loaded = loaded_plan(&host, &otp, &[27]);
    let mut references = ReferenceSet::new();
    references.add(
        Reference::Call("rabbit_amqqueue:lookup/1".parse().unwrap()),
        site("rabbitmq_shovel_extra"),
    );
    evaluate(&loaded, &plugin("rabbitmq_shovel_extra"), |_, _| {
        Ok::<_, Infallible>(references)
    })
    .unwrap()
}

fn host_indices(report: &JigsawReport) -> Vec<HostIdx> {
    report.targets().host_indices().collect()
}

#[test]
fn a_finding_at_releases_zero_one_two_and_four_gives_two_runs() {
    let report = report();
    let indices = host_indices(&report);
    let FindingSpan::Host(members) = &report.findings()[0].span else {
        panic!("a call is compared per host release");
    };
    let expected: BTreeSet<HostIdx> = [0, 1, 2, 4].into_iter().map(|p| indices[p]).collect();
    assert_eq!(members, &expected);
    let runs = TargetAxes::runs(members);
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].first, runs[0].last), (indices[0], indices[2]));
    assert_eq!((runs[1].first, runs[1].last), (indices[4], indices[4]));
}

#[test]
fn the_runs_of_a_set_cover_exactly_its_members() {
    let report = report();
    let indices = host_indices(&report);
    for members in [
        vec![],
        vec![0],
        vec![1, 2, 4],
        vec![0, 1, 2, 3, 4, 5],
        vec![0, 2, 4],
    ] {
        let members: BTreeSet<HostIdx> = members.into_iter().map(|p| indices[p]).collect();
        let covered: BTreeSet<HostIdx> = TargetAxes::runs(&members)
            .into_iter()
            .flat_map(|run| {
                indices
                    .iter()
                    .copied()
                    .filter(move |i| *i >= run.first && *i <= run.last)
            })
            .collect();
        assert_eq!(covered, members);
    }
}

#[test]
fn the_cell_of_a_broken_app_follows_the_span_and_other_apps_stay_clean() {
    let report = report();
    let indices = host_indices(&report);
    let otp = report.targets().otp_indices().next().unwrap();
    for (position, host_index) in indices.iter().enumerate() {
        let expected = if matches!(position, 0 | 1 | 2 | 4) {
            JigsawCell::Broken
        } else {
            JigsawCell::Clean
        };
        assert_eq!(
            report.cell(&app("rabbitmq_shovel_extra"), *host_index, otp),
            expected
        );
        assert_eq!(
            report.cell(&app("rabbit"), *host_index, otp),
            JigsawCell::Clean
        );
    }
}
