// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use bel7_cli::TableStyle;

use backhopper_cli::commands::jigsaw_render::{
    write_check_markdown, write_check_text, write_plan_markdown, write_plan_text,
};
use backhopper_core::jigsaw::findings::{JigsawFindingKind, JigsawWarningKind, WithheldReason};
use backhopper_core::jigsaw::plugin_set::AppRole;
use backhopper_core::jigsaw::references::NotChecked;
use backhopper_core::jigsaw::report::JigsawCell;
use backhopper_core::jigsaw::{
    JigsawAppRowPayload, JigsawCheckPayload, JigsawFindingPayload, JigsawPlanPayload,
    JigsawRangePayload, JigsawSitePayload, JigsawTalliesPayload, JigsawUncheckedPayload,
    JigsawWarningPayload, JigsawWithheldPayload, LIMITS, MissingSnapshotPayload,
    PlanOtpLinePayload, PlanReleasePayload, SnapshotGap, TagSourceKind,
};
use backhopper_core::model::names::{ApplicationName, ProjectName, RelativePath, TagName};
use backhopper_core::model::pin::Pin;

use JigsawCell::{Broken, Clean};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|&item| item.to_owned()).collect()
}

fn app_name(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn apps(names: &[&str]) -> Vec<ApplicationName> {
    names.iter().map(|&name| app_name(name)).collect()
}

fn file_path(path: &str) -> RelativePath {
    RelativePath::new(path).unwrap()
}

fn range(first: &str, last: &str) -> JigsawRangePayload {
    JigsawRangePayload {
        first: first.to_owned(),
        last: last.to_owned(),
    }
}

fn tallies(checked: u64) -> JigsawTalliesPayload {
    JigsawTalliesPayload {
        checked,
        not_checked: NotChecked::ALL.iter().map(|&reason| (reason, 0)).collect(),
        withheld: WithheldReason::ALL
            .iter()
            .map(|&reason| (reason, 0))
            .collect(),
    }
}

fn clean_payload(checked: u64, tag_source: TagSourceKind) -> JigsawCheckPayload {
    JigsawCheckPayload {
        host: "rabbitmq".to_owned(),
        series: "4.3".to_owned(),
        tag_source,
        build_release: "v4.3.2".to_owned(),
        host_releases: strings(&["v4.3.0", "v4.3.1", "v4.3.2"]),
        otp_tags: strings(&["OTP-27.3", "OTP-27.3.1"]),
        apps: vec![JigsawAppRowPayload {
            app: app_name("rabbitmq_shovel_extra"),
            role: AppRole::Plugin,
            cells: vec![Clean; 6],
        }],
        findings: Vec::new(),
        warnings: Vec::new(),
        host_provided: Vec::new(),
        unchecked: Vec::new(),
        withheld: Vec::new(),
        tallies: tallies(checked),
        limits: LIMITS.iter().map(|&limit| limit.to_owned()).collect(),
        store_digest: "0123456789abcdef".to_owned(),
        clean: true,
    }
}

fn unchecked(reason: NotChecked, symbol: &str, path: &str, line: u32) -> JigsawUncheckedPayload {
    JigsawUncheckedPayload {
        reason,
        symbol: symbol.to_owned(),
        app: app_name("rabbitmq_shovel_extra"),
        path: file_path(path),
        line,
    }
}

fn broken_payload() -> JigsawCheckPayload {
    let mut payload = clean_payload(1214, TagSourceKind::Repository);
    payload.apps = vec![
        JigsawAppRowPayload {
            app: app_name("rabbitmq_shovel_extra"),
            role: AppRole::Plugin,
            cells: vec![Broken, Broken, Broken, Broken, Clean, Clean],
        },
        JigsawAppRowPayload {
            app: app_name("amqp10_client"),
            role: AppRole::Bundled,
            cells: vec![Clean; 6],
        },
    ];
    payload.findings = vec![
        JigsawFindingPayload {
            kind: JigsawFindingKind::PartialMissing,
            symbol: "{rabbit, \"ssl_options\"}".to_owned(),
            detail: None,
            apps: apps(&["rabbitmq_shovel_extra"]),
            sites: vec![JigsawSitePayload {
                app: app_name("rabbitmq_shovel_extra"),
                path: file_path("priv/schema/rabbitmq_shovel_extra.schema"),
                line: 41,
            }],
            host_release_ranges: vec![range("v4.3.0", "v4.3.1")],
            otp_tag_ranges: Vec::new(),
        },
        JigsawFindingPayload {
            kind: JigsawFindingKind::FunctionMissing,
            symbol: "rabbit_amqqueue:lookup_many/2".to_owned(),
            detail: Some("target has /1".to_owned()),
            apps: apps(&["rabbitmq_shovel_extra"]),
            sites: vec![JigsawSitePayload {
                app: app_name("rabbitmq_shovel_extra"),
                path: file_path("src/rabbit_shovel_extra_worker.erl"),
                line: 118,
            }],
            host_release_ranges: vec![range("v4.3.0", "v4.3.0")],
            otp_tag_ranges: Vec::new(),
        },
    ];
    payload
        .tallies
        .not_checked
        .insert(NotChecked::Internal, 3870);
    payload.tallies.not_checked.insert(NotChecked::Dynamic, 2);
    payload
        .tallies
        .not_checked
        .insert(NotChecked::UnresolvedAtBuild, 1);
    payload.unchecked = vec![
        unchecked(
            NotChecked::Dynamic,
            "_:other_missing_helper/0",
            "src/rabbit_shovel_extra_worker.erl",
            57,
        ),
        unchecked(
            NotChecked::Dynamic,
            "rabbit_maintenance:drain/?",
            "src/rabbit_shovel_extra_worker.erl",
            92,
        ),
        unchecked(
            NotChecked::UnresolvedAtBuild,
            "rabbit_misc:no_such_helper/1",
            "src/rabbit_shovel_extra_worker.erl",
            40,
        ),
    ];
    payload
        .tallies
        .withheld
        .insert(WithheldReason::ConditionalMacro, 2);
    payload.withheld = vec![JigsawWithheldPayload {
        reason: WithheldReason::ConditionalMacro,
        symbol: "?DEFAULT_CONSUMER_TIMEOUT in include/rabbit.hrl".to_owned(),
        sites: vec![JigsawSitePayload {
            app: app_name("rabbitmq_shovel_extra"),
            path: file_path("src/rabbit_shovel_extra_worker.erl"),
            line: 13,
        }],
        host_release_ranges: vec![range("v4.3.1", "v4.3.1")],
        otp_tag_ranges: Vec::new(),
    }];
    payload.host_provided = apps(&["rabbit", "rabbit_common", "ra"]);
    payload.clean = false;
    payload
}

fn text(payload: &JigsawCheckPayload) -> String {
    let mut out = Vec::new();
    write_check_text(&mut out, payload, TableStyle::Ascii).unwrap();
    String::from_utf8(out).unwrap()
}

fn markdown(payload: &JigsawCheckPayload) -> String {
    let mut out = Vec::new();
    write_check_markdown(&mut out, payload).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn check_text_of_two_findings() {
    insta::assert_snapshot!(text(&broken_payload()));
}

#[test]
fn check_markdown_of_two_findings() {
    insta::assert_snapshot!(markdown(&broken_payload()));
}

#[test]
fn check_text_lists_each_unchecked_site_under_its_tally_phrase() {
    let out = text(&broken_payload());
    let file = "rabbitmq_shovel_extra/src/rabbit_shovel_extra_worker.erl";
    let unresolved =
        format!("not resolved at v4.3.2:\n  {file}:40  rabbit_misc:no_such_helper/1\n");
    let dynamic = format!(
        "dynamic calls:\n  {file}:57  _:other_missing_helper/0\n  {file}:92  rabbit_maintenance:drain/?\n"
    );
    assert!(out.contains(&unresolved), "{out}");
    assert!(out.contains(&dynamic), "{out}");
}

#[test]
fn check_text_names_each_withheld_comparison_with_its_releases_and_sites() {
    let out = text(&broken_payload());
    assert!(out.contains("comparisons withheld:\n"), "{out}");
    assert!(
        out.contains("conditional_macro  ?DEFAULT_CONSUMER_TIMEOUT in include/rabbit.hrl\n"),
        "{out}"
    );
    assert!(out.contains("host releases: v4.3.1"), "{out}");
    assert!(
        out.contains("rabbitmq_shovel_extra  src/rabbit_shovel_extra_worker.erl:13"),
        "{out}"
    );
}

#[test]
fn check_markdown_names_each_withheld_comparison() {
    let out = markdown(&broken_payload());
    assert!(out.contains("Comparisons withheld:\n"), "{out}");
    assert!(
        out.contains("#### conditional_macro `?DEFAULT_CONSUMER_TIMEOUT in include/rabbit.hrl`"),
        "{out}"
    );
}

#[test]
fn a_clean_report_lists_no_withheld_comparisons() {
    let out = text(&clean_payload(0, TagSourceKind::Repository));
    assert!(!out.contains("comparisons withheld"));
    assert!(
        !markdown(&clean_payload(0, TagSourceKind::Repository)).contains("Comparisons withheld")
    );
}

#[test]
fn the_limits_line_ends_with_the_pinned_versions_note() {
    let mut payload = clean_payload(0, TagSourceKind::Repository);
    payload.limits = LIMITS.iter().map(|&limit| limit.to_owned()).collect();
    let out = text(&payload);
    assert!(
        out.trim_end().ends_with(
            "dependencies are checked at the exact versions each host release pins, not over a range"
        ),
        "{out}"
    );
}

#[test]
fn check_text_of_a_clean_report_has_no_unchecked_listing() {
    let out = text(&clean_payload(0, TagSourceKind::Repository));
    assert!(!out.contains("not resolved at v4.3.2:\n"));
    assert!(!out.contains("dynamic calls:\n"));
}

#[test]
fn check_markdown_lists_each_unchecked_site_as_a_code_span() {
    let out = markdown(&broken_payload());
    assert!(out.contains("not resolved at v4.3.2:\n\n* `rabbitmq_shovel_extra/src/rabbit_shovel_extra_worker.erl:40  rabbit_misc:no_such_helper/1`\n"), "{out}");
}

#[test]
fn check_text_of_a_clean_report_still_prints_the_references_line() {
    let out = text(&clean_payload(0, TagSourceKind::Repository));
    assert!(out.contains("0 distinct references to the host and OTP, checked at every target"));
    assert!(out.contains("0 findings"));
    assert_eq!(
        out.matches(": 0\n").count(),
        NotChecked::ALL.len() + WithheldReason::ALL.len()
    );
}

#[test]
fn check_markdown_of_a_clean_report_still_prints_the_references_line() {
    let out = markdown(&clean_payload(0, TagSourceKind::Repository));
    assert!(out.contains("0 distinct references to the host and OTP, checked at every target"));
    assert!(out.contains("* conditional macros: 0\n"));
}

#[test]
fn a_clean_matrix_is_one_column() {
    let out = text(&clean_payload(5, TagSourceKind::Repository));
    assert!(out.contains("v4.3.0 .. v4.3.2"), "{out}");
    assert_eq!(out.matches("clean").count(), 1);
}

#[test]
fn matrix_columns_split_by_otp_tags_only_when_the_state_differs() {
    let mut payload = clean_payload(5, TagSourceKind::Repository);
    payload.apps[0].cells = vec![Clean, Clean, Clean, Broken, Clean, Broken];
    let out = text(&payload);
    assert!(out.contains("v4.3.0, OTP-27.3 "), "{out}");
    assert!(out.contains("v4.3.1 .. v4.3.2, OTP-27.3.1"), "{out}");
}

#[test]
fn a_store_listing_says_so_on_the_targets_line() {
    let out = text(&clean_payload(5, TagSourceKind::Store));
    assert!(out.contains("host releases listed from the snapshot store"));
    let out = text(&clean_payload(5, TagSourceKind::Repository));
    assert!(!out.contains("snapshot store"));
}

#[test]
fn counts_use_thousands_separators_and_singular_nouns() {
    let mut payload = broken_payload();
    payload.findings.truncate(1);
    let out = text(&payload);
    assert!(out.contains("1 finding\n"));
    assert!(out.contains("1,214 distinct"));
    assert!(out.contains("call sites resolved inside the plugin set: 3,870"));
}

#[test]
fn warnings_and_host_provided_apps_are_listed() {
    let mut payload = broken_payload();
    payload.warnings = vec![JigsawWarningPayload {
        kind: JigsawWarningKind::RecordDefaultChanged,
        symbol: "#amqqueue.durable".to_owned(),
        detail: Some("build: true; target: false".to_owned()),
        host_release_ranges: vec![range("v4.3.2", "v4.3.2")],
    }];
    let out = text(&payload);
    assert!(out.contains("1 warning\n"));
    assert!(
        out.contains("record_default_changed  #amqqueue.durable  (build: true; target: false)")
    );
    assert!(
        out.contains("host apps found in plugin deps/ and left out: rabbit, rabbit_common, ra")
    );
}

#[test]
fn an_everywhere_finding_names_every_target() {
    let mut payload = broken_payload();
    payload.findings[0].host_release_ranges.clear();
    assert!(text(&payload).contains("  every target"));
}

#[test]
fn an_otp_finding_names_its_tags() {
    let mut payload = broken_payload();
    payload.findings[0].host_release_ranges.clear();
    payload.findings[0].otp_tag_ranges = vec![range("OTP-27.3", "OTP-27.3.1")];
    assert!(text(&payload).contains("OTP tags: OTP-27.3 .. OTP-27.3.1  (all host releases)"));
}

#[test]
fn a_cell_the_payload_lacks_is_unknown_and_not_clean() {
    let mut payload = clean_payload(5, TagSourceKind::Repository);
    payload.apps[0].cells.clear();
    let out = text(&payload);
    assert!(out.contains("unknown"), "{out}");
    assert!(!out.contains("clean"), "{out}");
}

#[test]
fn markdown_cells_escape_pipes() {
    let mut payload = clean_payload(5, TagSourceKind::Repository);
    payload.host_releases = strings(&["v4.3.0|rc"]);
    payload.apps[0].cells.truncate(1);
    assert!(markdown(&payload).contains("v4.3.0\\|rc"));
}

fn plan_payload() -> JigsawPlanPayload {
    let release = |tag: &str, ra: &str, support: &str| PlanReleasePayload {
        tag: tag.to_owned(),
        commit: "0".repeat(40),
        pins: BTreeMap::from([("ra".to_owned(), ra.to_owned())]),
        erlang_support: support.to_owned(),
    };
    JigsawPlanPayload {
        host: "rabbitmq".to_owned(),
        host_project: "rabbitmq-server".to_owned(),
        otp_project: "otp".to_owned(),
        series: "4.2".to_owned(),
        tag_source: "repository".to_owned(),
        extractor_version: "7".to_owned(),
        host_facts_digest: "abcd".to_owned(),
        releases: vec![
            release("v4.2.0", "v2.17.1", "[26.2,29.0)"),
            release("v4.2.1", "v2.17.1", "[26.2,29.0)"),
            release("v4.2.2", "v2.17.3", "[27.0,29.0)"),
        ],
        otp_lines: vec![PlanOtpLinePayload {
            line: "27".to_owned(),
            tags: strings(&["OTP-27.3", "OTP-27.3.1"]),
        }],
        untracked_dependencies: strings(&["cowlib"]),
        excluded_otp_apps: Vec::new(),
        required_snapshots: vec![Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.17.3").unwrap(),
        )],
        missing_snapshots: vec![MissingSnapshotPayload {
            project: "ra".to_owned(),
            tag: "v2.17.3".to_owned(),
            reason: SnapshotGap::Missing,
            detail: None,
        }],
        cache_key: "jigsaw-rabbitmq-4.2-abcd".to_owned(),
        restore_key: "jigsaw-rabbitmq-4.2-".to_owned(),
    }
}

#[test]
fn plan_text_snapshot() {
    let mut out = Vec::new();
    write_plan_text(&mut out, &plan_payload(), TableStyle::Ascii).unwrap();
    insta::assert_snapshot!(String::from_utf8(out).unwrap());
}

#[test]
fn plan_markdown_snapshot() {
    let mut out = Vec::new();
    write_plan_markdown(&mut out, &plan_payload()).unwrap();
    insta::assert_snapshot!(String::from_utf8(out).unwrap());
}
