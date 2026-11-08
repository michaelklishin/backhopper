// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::jigsaw::evaluate;
use backhopper_core::jigsaw::findings::Finding;
use backhopper_core::jigsaw::references::NotChecked;
use backhopper_core::jigsaw::report::{FindingSpan, JigsawReport};
use backhopper_jigsaw::collect;
use backhopper_test_support::{
    SnapshotBuilder, defining, hrl, including, loaded_plan, module_with,
};

use crate::support::{AppSpec, plugin_set};

const RELEASES: [&str; 3] = ["v4.2.0", "v4.2.1", "v4.2.2"];

fn base(tag: &str) -> SnapshotBuilder {
    SnapshotBuilder::new("rabbitmq-server", tag)
        .roots(&["rabbit"])
        .app("rabbit", &["rabbit_common"])
        .app("rabbit_common", &[])
}

fn kernel(tag: &str) -> SnapshotBuilder {
    SnapshotBuilder::new("otp", tag).app("kernel", &[])
}

fn check(
    host: Vec<(&'static str, SnapshotBuilder)>,
    otp: &[(&str, SnapshotBuilder)],
    majors: &[u32],
    worker: &str,
) -> JigsawReport {
    check_files(
        host,
        otp,
        majors,
        &[("src/rabbit_shovel_extra_worker.erl", worker)],
    )
}

fn check_files(
    host: Vec<(&'static str, SnapshotBuilder)>,
    otp: &[(&str, SnapshotBuilder)],
    majors: &[u32],
    files: &[(&str, &str)],
) -> JigsawReport {
    let loaded = loaded_plan(&host, otp, majors);
    let plugins = plugin_set(&[AppSpec {
        name: "rabbitmq_shovel_extra",
        requires: &["rabbit_common"],
        files,
    }]);
    evaluate(&loaded, &plugins, collect).unwrap()
}

fn json_available_from_28() -> [(&'static str, SnapshotBuilder); 2] {
    [
        ("OTP-27.3", kernel("OTP-27.3")),
        (
            "OTP-28.5",
            kernel("OTP-28.5").module("kernel", module_with("json", &[("encode", 1)])),
        ),
    ]
}

fn plain_host() -> Vec<(&'static str, SnapshotBuilder)> {
    RELEASES.iter().map(|tag| (*tag, base(tag))).collect()
}

#[test]
fn an_otp_28_export_used_unguarded_is_a_finding_on_the_27_line() {
    let report = check(
        plain_host(),
        &json_available_from_28(),
        &[27, 28],
        "-module(rabbit_shovel_extra_worker).\nrun() -> json:encode(1).\n",
    );
    assert_eq!(report.findings().len(), 1);
    assert!(matches!(
        &report.findings()[0].finding,
        Finding::ModuleMissing { module } if module.as_str() == "json"
    ));
    assert!(matches!(report.findings()[0].span, FindingSpan::Otp(_)));
}

#[test]
fn the_same_call_under_a_version_conditional_is_withheld() {
    let report = check(
        plain_host(),
        &json_available_from_28(),
        &[27, 28],
        "-module(rabbit_shovel_extra_worker).\n-if(?OTP_RELEASE >= 28).\nrun() -> json:encode(1).\n-else.\nrun() -> ok.\n-endif.\n",
    );
    assert!(report.is_clean());
    assert_eq!(
        report.tallies().not_checked()[&NotChecked::ConditionalSite],
        1
    );
}

#[test]
fn a_macro_defined_two_headers_away_is_compared_at_the_header_that_defines_it() {
    let host = RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let outer = including(
                hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                "rabbit_common",
                "include/resource.hrl",
            );
            let limit = if index == 0 { "128" } else { "255" };
            let inner = defining(
                hrl("rabbit_common", "deps/rabbit_common/include/resource.hrl"),
                "ROUTING_KEY_LIMIT",
                None,
                limit,
            );
            (*tag, base(tag).header(outer).header(inner))
        })
        .collect();
    let report = check(
        host,
        &[("OTP-27.3", kernel("OTP-27.3"))],
        &[27],
        "-module(rabbit_shovel_extra_worker).\n-include_lib(\"rabbit_common/include/rabbit.hrl\").\nlimit() -> ?ROUTING_KEY_LIMIT.\n",
    );
    assert_eq!(report.findings().len(), 1);
    assert!(matches!(
        &report.findings()[0].finding,
        Finding::MacroChanged { header, .. } if header.app_path.as_str() == "include/resource.hrl"
    ));
}

#[test]
fn a_call_inside_a_host_macro_is_checked_at_every_release() {
    let host = RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let header = defining(
                hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                "RABBIT_LOG",
                Some(2),
                "rabbit_log:log(A1, A2)",
            );
            let exports: &[(&str, u8)] = if index == 0 { &[] } else { &[("log", 2)] };
            (
                *tag,
                base(tag)
                    .header(header)
                    .module("rabbit", module_with("rabbit_log", exports)),
            )
        })
        .collect();
    let report = check(
        host,
        &[("OTP-27.3", kernel("OTP-27.3"))],
        &[27],
        "-module(rabbit_shovel_extra_worker).\n-include_lib(\"rabbit_common/include/rabbit.hrl\").\nrun() -> ?RABBIT_LOG(info, \"started\").\n",
    );
    let found: Vec<_> = report
        .findings()
        .iter()
        .map(|ranged| &ranged.finding)
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(matches!(
        found[0],
        Finding::FunctionMissing { mfa, .. } if mfa.to_string() == "rabbit_log:log/2"
    ));
}

#[test]
fn a_plugin_that_only_calls_what_every_release_exports_is_clean() {
    let host = RELEASES
        .iter()
        .map(|tag| {
            (
                *tag,
                base(tag).module("rabbit", module_with("rabbit_amqqueue", &[("lookup", 1)])),
            )
        })
        .collect();
    let report = check(
        host,
        &[("OTP-27.3", kernel("OTP-27.3"))],
        &[27],
        "-module(rabbit_shovel_extra_worker).\nrun(Q) -> rabbit_amqqueue:lookup(Q).\n",
    );
    assert!(report.is_clean());
    assert_eq!(report.tallies().checked(), 3);
}

#[test]
fn a_host_macro_used_in_a_plugin_header_is_compared_and_sited_at_the_header_line() {
    let host = RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let limit = if index == 0 { "128" } else { "255" };
            let header = defining(
                hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                "ROUTING_KEY_LIMIT",
                None,
                limit,
            );
            (*tag, base(tag).header(header))
        })
        .collect();
    let report = check_files(
        host,
        &[("OTP-27.3", kernel("OTP-27.3"))],
        &[27],
        &[
            (
                "src/rabbit_shovel_extra_worker.erl",
                "-module(rabbit_shovel_extra_worker).\n-include_lib(\"rabbit_common/include/rabbit.hrl\").\n-include(\"limits.hrl\").\n",
            ),
            (
                "include/limits.hrl",
                "%% limits\n-define(LIMIT, ?ROUTING_KEY_LIMIT).\n",
            ),
        ],
    );
    assert_eq!(report.findings().len(), 1);
    let ranged = &report.findings()[0];
    assert!(matches!(&ranged.finding, Finding::MacroChanged { .. }));
    assert_eq!(
        ranged.sites.first().file.path.as_str(),
        "include/limits.hrl"
    );
    assert_eq!(ranged.sites.first().line, 2);
}
