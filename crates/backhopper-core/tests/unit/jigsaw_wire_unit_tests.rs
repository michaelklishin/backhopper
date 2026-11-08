// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::path::PathBuf;

use serde_json::Value;

use backhopper_core::jigsaw::findings::{JigsawFindingKind, WithheldReason};
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginFile, PluginSet, PluginSources};
use backhopper_core::jigsaw::references::{
    DynamicCall, NotChecked, Reference, ReferenceSet, ReferenceSite, SiteDetail, UncheckedSite,
    UncheckedSymbol,
};
use backhopper_core::jigsaw::report::{JigsawCell, JigsawReport};
use backhopper_core::jigsaw::{JigsawCheckPayload, LIMITS, evaluate};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::names::{ApplicationName, Arity, Mfa, RelativePath};
use backhopper_test_support::{SnapshotBuilder, loaded_plan, module_with, with_spec};

const RELEASES: [&str; 4] = ["v4.2.0", "v4.2.1", "v4.2.2", "v4.2.3"];
const APP: &str = "rabbitmq_shovel_extra";

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn host_release(tag: &str, lookup_many_arity: u8) -> (&str, SnapshotBuilder) {
    let builder = SnapshotBuilder::new("rabbitmq-server", tag)
        .roots(&["rabbit"])
        .app("rabbit", &["rabbit_common"])
        .app("rabbit_common", &[])
        .module(
            "rabbit",
            module_with(
                "rabbit_amqqueue",
                &[("lookup", 1), ("lookup_many", lookup_many_arity)],
            ),
        );
    (tag, builder)
}

fn kernel(tag: &str) -> SnapshotBuilder {
    SnapshotBuilder::new("otp", tag)
        .app("kernel", &[])
        .module("kernel", module_with("lists", &[("map", 2)]))
}

fn plugin() -> PluginSet {
    PluginSet::new(PluginSources {
        apps: vec![PluginAppSources {
            root: PathBuf::from(format!("/plugins/{APP}")),
            manifest: AppManifest {
                name: app(APP),
                source: ManifestSource::AppSrc,
                requires: ManifestDeps::Listed(BTreeSet::new()),
            }
            .into_read()
            .unwrap(),
            manifest_path: RelativePath::new(format!("src/{APP}.app.src")).unwrap(),
            files: Vec::new(),
        }],
        host_provided: BTreeSet::new(),
    })
    .unwrap()
}

fn report(fixed_at: usize) -> JigsawReport {
    report_with(fixed_at, |_| {})
}

fn report_with(fixed_at: usize, extend: impl FnOnce(&mut ReferenceSet)) -> JigsawReport {
    let host: Vec<_> = RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| host_release(tag, if index >= fixed_at { 2 } else { 1 }))
        .collect();
    let otp = [
        ("OTP-27.3", kernel("OTP-27.3")),
        ("OTP-27.3.1", kernel("OTP-27.3.1")),
    ];
    let mut references = ReferenceSet::new();
    references.add(
        Reference::Call("rabbit_amqqueue:lookup_many/2".parse::<Mfa>().unwrap()),
        ReferenceSite {
            file: PluginFile {
                app: app(APP),
                path: RelativePath::new("src/rabbit_shovel_extra_worker.erl").unwrap(),
            },
            line: 118,
            detail: SiteDetail::Plain,
        },
    );
    extend(&mut references);
    let loaded = loaded_plan(&host, &otp, &[27]);
    evaluate(&loaded, &plugin(), |_, _| Ok::<_, Infallible>(references)).unwrap()
}

#[test]
fn clean_is_set_from_the_findings() {
    let broken = JigsawCheckPayload::from(&report(2));
    assert!(!broken.clean);
    assert_eq!(broken.findings.len(), 1);
    assert_eq!(broken.findings[0].kind, JigsawFindingKind::FunctionMissing);

    let clean = JigsawCheckPayload::from(&report(0));
    assert!(clean.clean);
    assert!(clean.findings.is_empty());
}

#[test]
fn cells_are_row_major_over_host_releases_and_otp_tags() {
    let payload = JigsawCheckPayload::from(&report(2));
    assert_eq!(payload.host_releases, RELEASES);
    assert_eq!(payload.otp_tags, ["OTP-27.3", "OTP-27.3.1"]);
    assert_eq!(payload.apps.len(), 1);
    assert_eq!(payload.apps[0].app, APP);
    let (broken, clean) = (JigsawCell::Broken, JigsawCell::Clean);
    assert_eq!(
        payload.apps[0].cells,
        [broken, broken, broken, broken, clean, clean, clean, clean]
    );
}

#[test]
fn a_finding_carries_its_runs_and_sites() {
    let payload = JigsawCheckPayload::from(&report(2));
    let finding = &payload.findings[0];
    assert_eq!(finding.symbol, "rabbit_amqqueue:lookup_many/2");
    assert_eq!(finding.detail.as_deref(), Some("target has /1"));
    assert_eq!(finding.host_release_ranges.len(), 1);
    assert_eq!(finding.host_release_ranges[0].first, "v4.2.0");
    assert_eq!(finding.host_release_ranges[0].last, "v4.2.1");
    assert!(finding.otp_tag_ranges.is_empty());
    assert_eq!(finding.sites[0].path, "src/rabbit_shovel_extra_worker.erl");
    assert_eq!(finding.sites[0].line, 118);
}

#[test]
fn every_tally_label_is_present_and_limits_are_fixed() {
    let payload = JigsawCheckPayload::from(&report(0));
    assert_eq!(payload.tallies.not_checked.len(), NotChecked::ALL.len());
    assert_eq!(payload.tallies.withheld.len(), WithheldReason::ALL.len());
    assert_eq!(payload.limits, LIMITS);
    assert_eq!(payload.build_release, "v4.2.3");
}

#[test]
fn the_wire_form_keys_tallies_by_label_and_round_trips() {
    let payload = JigsawCheckPayload::from(&report(2));
    let json = serde_json::to_value(&payload).unwrap();
    let not_checked = json["tallies"]["not_checked"].as_object().unwrap();
    assert!(not_checked.contains_key("unresolved_at_build"));
    let withheld = json["tallies"]["withheld"].as_object().unwrap();
    assert!(withheld.contains_key("conditional_macro"));
    assert_eq!(json["apps"][0]["cells"][0], Value::from("broken"));
    let back: JigsawCheckPayload = serde_json::from_value(json).unwrap();
    assert_eq!(back, payload);
}

fn worker_site(line: u32) -> ReferenceSite {
    ReferenceSite {
        file: PluginFile {
            app: app(APP),
            path: RelativePath::new("src/rabbit_shovel_extra_worker.erl").unwrap(),
        },
        line,
        detail: SiteDetail::Plain,
    }
}

fn report_with_unchecked() -> JigsawReport {
    report_with(0, |references| {
        references.add(
            Reference::Call("rabbit_misc:no_such_helper/1".parse::<Mfa>().unwrap()),
            worker_site(40),
        );
        let site = worker_site(55);
        references.add_unchecked(UncheckedSite {
            symbol: UncheckedSymbol::Dynamic(DynamicCall {
                module: None,
                function: Some("other_missing_helper".parse().unwrap()),
                arity: Some(Arity::new(0)),
            }),
            file: site.file,
            line: site.line,
        });
    })
}

#[test]
fn unchecked_sites_are_listed_with_reason_symbol_file_and_line() {
    let payload = JigsawCheckPayload::from(&report_with_unchecked());
    let listed: Vec<(NotChecked, &str, &str, &str, u32)> = payload
        .unchecked
        .iter()
        .map(|site| {
            (
                site.reason,
                site.symbol.as_str(),
                site.app.as_str(),
                site.path.as_str(),
                site.line,
            )
        })
        .collect();
    let path = "src/rabbit_shovel_extra_worker.erl";
    assert_eq!(
        listed,
        vec![
            (
                NotChecked::Dynamic,
                "_:other_missing_helper/0",
                APP,
                path,
                55
            ),
            (
                NotChecked::UnresolvedAtBuild,
                "rabbit_misc:no_such_helper/1",
                APP,
                path,
                40
            ),
        ]
    );
}

#[test]
fn a_listed_reason_is_counted_as_often_as_it_is_listed() {
    let payload = JigsawCheckPayload::from(&report_with_unchecked());
    for reason in [NotChecked::Dynamic, NotChecked::UnresolvedAtBuild] {
        let listed = payload
            .unchecked
            .iter()
            .filter(|s| s.reason == reason)
            .count();
        assert_eq!(payload.tallies.not_checked[&reason], listed as u64);
    }
}

#[test]
fn the_unchecked_list_round_trips_and_is_empty_for_a_clean_run() {
    let payload = JigsawCheckPayload::from(&report_with_unchecked());
    let json = serde_json::to_value(&payload).unwrap();
    assert_eq!(json["unchecked"][0]["reason"], Value::from("dynamic"));
    assert_eq!(json["unchecked"][1]["line"], Value::from(40));
    let back: JigsawCheckPayload = serde_json::from_value(json).unwrap();
    assert_eq!(back, payload);
    assert!(JigsawCheckPayload::from(&report(0)).unchecked.is_empty());
}

fn unknown_spec_report() -> JigsawReport {
    let host: Vec<_> = RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let spec = if index == 1 {
                "an unparsable spec"
            } else {
                "(term()) -> {ok, pid()}"
            };
            let builder = SnapshotBuilder::new("rabbitmq-server", tag)
                .roots(&["rabbit"])
                .app("rabbit", &["rabbit_common"])
                .app("rabbit_common", &[])
                .module(
                    "rabbit_common",
                    with_spec(
                        module_with("rabbit_misc", &[("start", 1)]),
                        "start",
                        1,
                        spec,
                    ),
                );
            (*tag, builder)
        })
        .collect();
    let otp = [("OTP-27.3", kernel("OTP-27.3"))];
    let mut references = ReferenceSet::new();
    for line in [31, 54] {
        references.add(
            Reference::Call("rabbit_misc:start/1".parse::<Mfa>().unwrap()),
            ReferenceSite {
                file: PluginFile {
                    app: app(APP),
                    path: RelativePath::new("src/rabbit_shovel_extra_worker.erl").unwrap(),
                },
                line,
                detail: SiteDetail::Plain,
            },
        );
    }
    let loaded = loaded_plan(&host, &otp, &[27]);
    evaluate(&loaded, &plugin(), |_, _| Ok::<_, Infallible>(references)).unwrap()
}

#[test]
fn a_withheld_comparison_carries_its_symbol_sites_and_releases() {
    let payload = JigsawCheckPayload::from(&unknown_spec_report());
    let [withheld] = payload.withheld.as_slice() else {
        panic!("one withheld symbol expected");
    };
    assert_eq!(withheld.reason, WithheldReason::UnknownSpec);
    assert_eq!(withheld.symbol, "rabbit_misc:start/1");
    assert_eq!(withheld.sites.len(), 2);
    assert_eq!(withheld.sites[0].line, 31);
    assert_eq!(withheld.host_release_ranges.len(), 1);
    assert_eq!(withheld.host_release_ranges[0].first, "v4.2.1");
    assert_eq!(withheld.host_release_ranges[0].last, "v4.2.1");
    assert!(withheld.otp_tag_ranges.is_empty());
}

#[test]
fn the_tally_of_a_listed_withheld_reason_is_the_sum_of_its_sites() {
    let payload = JigsawCheckPayload::from(&unknown_spec_report());
    let listed: usize = payload
        .withheld
        .iter()
        .filter(|entry| entry.reason == WithheldReason::UnknownSpec)
        .map(|entry| entry.sites.len())
        .sum();
    assert_eq!(
        payload.tallies.withheld[&WithheldReason::UnknownSpec],
        listed as u64
    );
}
