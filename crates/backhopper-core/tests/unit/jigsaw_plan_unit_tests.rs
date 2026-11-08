// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use backhopper_core::config::Config;
use backhopper_core::jigsaw::{
    DraftedPlan, JigsawPlan, ListedTags, PlanError, ResolvedRelease, SeriesRequest, SeriesTags,
    StoreStatus, TagSourceKind, plan_series, select_otp_lines, select_series_tags,
};
use backhopper_core::model::names::TagName;
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::{ErlangMajor, HostSeries};

use crate::jigsaw_support::{
    CONFIG, commit, config, config_at, extractor, hex_pin, rabbitmq, tag, tags,
};

const HOST_TAGS: &[&str] = &[
    "v4.2.0",
    "v4.2.1",
    "v4.2.2",
    "v4.2.3",
    "v4.2.4",
    "v4.2.5",
    "v4.2.6",
    "v4.2.7",
    "v4.2.8",
    "v4.2.0-rc.1",
    "v4.2.x",
    "v4.3.0",
    "v3.13.7",
];

const OTP_TAGS: &[&str] = &[
    "OTP-26.2.5",
    "OTP-27.0",
    "OTP-27.3",
    "OTP-27.3.1",
    "OTP-27.3.4.18",
    "OTP-28.5",
    "OTP-28.5.0.7",
    "OTP-29.1",
    "OTP-29.1.1",
    "OTP-17.0-rc1",
    "OTP_R16B03",
];

fn series(value: &str) -> HostSeries {
    value.parse().unwrap()
}

fn majors(values: &[u32]) -> NonEmpty<ErlangMajor> {
    let majors: Vec<ErlangMajor> = values
        .iter()
        .map(|value| ErlangMajor::try_from(*value).unwrap())
        .collect();
    NonEmpty::try_from(majors).unwrap()
}

fn versions(selected: &SeriesTags) -> Vec<String> {
    selected
        .releases
        .iter()
        .map(|release| release.version().to_string())
        .collect()
}

fn select(cfg: &Config, up_to: Option<&TagName>) -> Result<SeriesTags, PlanError> {
    let host = rabbitmq(cfg);
    let project = cfg.project(&host.project).unwrap();
    select_series_tags(
        project,
        &host.series_tag_pattern,
        series("4.2"),
        up_to,
        &tags(HOST_TAGS),
    )
}

fn draft(cfg: &Config, requested: &[u32]) -> Result<DraftedPlan, PlanError> {
    let host = rabbitmq(cfg);
    plan_series(
        cfg,
        host,
        &SeriesRequest {
            series: series("4.2"),
            up_to: None,
            majors: majors(requested),
        },
        &ListedTags {
            host: tags(HOST_TAGS),
            otp: tags(OTP_TAGS),
        },
        TagSourceKind::Repository,
        extractor(),
        |release| {
            let ra = if release.version().to_string().as_str() < "4.2.5" {
                "2.17.1"
            } else {
                "2.17.3"
            };
            Ok(ResolvedRelease {
                commit: commit('a'),
                pins: vec![hex_pin("ra", ra), hex_pin("jose", "1.11.10")],
            })
        },
    )
}

#[test]
fn series_tags_keep_the_patch_releases_of_the_series() {
    let selected = select(&config(), None).unwrap();
    assert_eq!(
        versions(&selected),
        [
            "4.2.0", "4.2.1", "4.2.2", "4.2.3", "4.2.4", "4.2.5", "4.2.6", "4.2.7", "4.2.8"
        ]
    );
}

#[test]
fn a_tag_that_matches_the_pattern_but_is_not_a_version_is_listed_as_skipped() {
    let selected = select(&config(), None).unwrap();
    assert_eq!(selected.skipped_tags, vec![tag("v4.2.x")]);
}

#[test]
fn up_to_cuts_the_series_at_that_release() {
    let selected = select(&config(), Some(&tag("v4.2.5"))).unwrap();
    assert_eq!(selected.releases.build().tag().as_str(), "v4.2.5");
    assert_eq!(versions(&selected).len(), 6);
}

#[test]
fn up_to_must_be_a_release_of_the_series() {
    for outside in ["v4.3.0", "v4.2.0-rc.1", "v4.2.99"] {
        let error = select(&config(), Some(&tag(outside))).unwrap_err();
        assert!(
            matches!(error, PlanError::UpToNotInSeries { .. }),
            "{outside}: {error}"
        );
    }
}

#[test]
fn a_series_with_no_releases_is_an_empty_series_error() {
    let cfg = config();
    let host = rabbitmq(&cfg);
    let project = cfg.project(&host.project).unwrap();
    let error = select_series_tags(
        project,
        &host.series_tag_pattern,
        series("9.9"),
        None,
        &tags(HOST_TAGS),
    )
    .unwrap_err();
    assert!(matches!(error, PlanError::EmptySeries { .. }));
}

#[test]
fn otp_lines_are_the_newest_minor_line_of_each_major() {
    let cfg = config();
    let otp = cfg.project(&rabbitmq(&cfg).otp_project).unwrap();
    let lines = select_otp_lines(otp, &majors(&[27, 28, 29]), &tags(OTP_TAGS)).unwrap();
    let described: Vec<(String, Vec<String>)> = lines
        .iter()
        .map(|line| {
            (
                line.line().to_string(),
                line.tags().iter().map(|t| t.tag().to_string()).collect(),
            )
        })
        .collect();
    assert_eq!(described[0].0, "27.3");
    assert_eq!(described[0].1, ["OTP-27.3", "OTP-27.3.1", "OTP-27.3.4.18"]);
    assert_eq!(described[1].0, "28.5");
    assert_eq!(described[1].1, ["OTP-28.5", "OTP-28.5.0.7"]);
    assert_eq!(described[2].0, "29.1");
    assert_eq!(described[2].1, ["OTP-29.1", "OTP-29.1.1"]);
}

#[test]
fn a_major_with_no_tags_is_an_error_naming_it() {
    let cfg = config();
    let otp = cfg.project(&rabbitmq(&cfg).otp_project).unwrap();
    let error = select_otp_lines(otp, &majors(&[27, 31]), &tags(OTP_TAGS)).unwrap_err();
    assert!(
        matches!(&error, PlanError::NoOtpTags { major } if major.get() == 31),
        "{error}"
    );
}

#[test]
fn a_plan_pairs_each_release_with_its_range_and_pins() {
    let drafted = draft(&config(), &[27, 28]).unwrap();
    let plan = drafted.plan;
    assert_eq!(plan.series().to_string(), "4.2");
    assert_eq!(plan.tag_source(), TagSourceKind::Repository);
    let first = plan.releases().iter().next().unwrap();
    assert_eq!(first.erlang_support.to_string(), "[26.2,29.0)");
    assert_eq!(first.pins.len(), 1);
    assert_eq!(plan.untracked_dependencies().len(), 1);
    assert_eq!(plan.excluded_otp_apps().len(), 10);
    assert_eq!(drafted.skipped_tags, vec![tag("v4.2.x")]);
}

#[test]
fn required_snapshots_are_deduplicated_across_releases() {
    let plan = draft(&config(), &[27, 28]).unwrap().plan;
    let required: Vec<String> = plan.required().iter().map(ToString::to_string).collect();
    let ra: Vec<&String> = required
        .iter()
        .filter(|pin| pin.starts_with("ra@"))
        .collect();
    assert_eq!(ra.len(), 2);
    let host: Vec<&String> = required
        .iter()
        .filter(|pin| pin.starts_with("rabbitmq-server@"))
        .collect();
    assert_eq!(host.len(), 9);
}

#[test]
fn a_release_that_does_not_support_a_requested_major_fails_the_plan() {
    let error = draft(&config(), &[27, 28, 29]).unwrap_err();
    let PlanError::ErlangMajorUnsupported { failures } = &error else {
        panic!("expected an unsupported major, got {error}");
    };
    assert!(failures.iter().all(|f| f.line.to_string() == "29.1"));
    let message = error.to_string();
    assert!(message.contains("v4.2.0"), "{message}");
    assert!(message.contains("29.1"), "{message}");
}

#[test]
fn the_cache_key_carries_the_host_series_format_and_extractor() {
    let plan = draft(&config(), &[27, 28]).unwrap().plan;
    let key = plan.cache_key();
    assert!(
        key.as_str().starts_with("jigsaw-rabbitmq-4.2-f5-e9-"),
        "{key}"
    );
    assert_eq!(key.restore_prefix(), "jigsaw-rabbitmq-4.2-f5-e9-");
    assert_eq!(key.as_str().len(), key.restore_prefix().len() + 16);
}

#[test]
fn the_cache_key_changes_with_the_majors_and_the_host_facts() {
    let two = draft(&config(), &[27, 28]).unwrap().plan;
    let one = draft(&config(), &[27]).unwrap().plan;
    assert_ne!(two.cache_key(), one.cache_key());
    let other_pins = CONFIG.replace("rabbitmq-components.mk", "deps.mk");
    let drifted = draft(
        &config_at(Path::new("/tmp/backhopper.toml"), &other_pins),
        &[27, 28],
    )
    .unwrap()
    .plan;
    assert_ne!(two.host_facts(), drifted.host_facts());
    assert_ne!(two.cache_key(), drifted.cache_key());
}

#[test]
fn from_wire_gives_back_the_plan_that_was_written() {
    let cfg = config();
    let plan = draft(&cfg, &[27, 28]).unwrap().plan;
    let payload = plan.to_wire(&plan.nothing_stored());
    assert_eq!(JigsawPlan::from_wire(&payload, &cfg).unwrap(), plan);
}

#[test]
fn from_wire_recomputes_the_cache_key_and_ignores_the_derived_fields() {
    let cfg = config();
    let plan = draft(&cfg, &[27, 28]).unwrap().plan;
    let mut payload = plan.to_wire(&StoreStatus::Stocked);
    payload.cache_key = "jigsaw-edited-0000000000000000".to_owned();
    payload.restore_key = "edited".to_owned();
    payload.required_snapshots.clear();
    payload.missing_snapshots.clear();
    let read = JigsawPlan::from_wire(&payload, &cfg).unwrap();
    assert_eq!(read.cache_key(), plan.cache_key());
}

#[test]
fn from_wire_refuses_a_range_edited_to_exclude_a_requested_line() {
    let cfg = config();
    let plan = draft(&cfg, &[27, 28]).unwrap().plan;
    let mut payload = plan.to_wire(&StoreStatus::Stocked);
    payload.releases[0].erlang_support = "[27.0,28.0)".to_owned();
    let error = JigsawPlan::from_wire(&payload, &cfg).unwrap_err();
    assert!(
        matches!(error, PlanError::ErlangMajorUnsupported { .. }),
        "{error}"
    );
}

#[test]
fn from_wire_refuses_values_the_newtypes_refuse() {
    let cfg = config();
    let plan = draft(&cfg, &[27, 28]).unwrap().plan;
    let mut payload = plan.to_wire(&StoreStatus::Stocked);
    payload.releases[0].commit = "abc".to_owned();
    assert!(matches!(
        JigsawPlan::from_wire(&payload, &cfg),
        Err(PlanError::Wire {
            field: "releases.commit",
            ..
        })
    ));
    let mut payload = plan.to_wire(&StoreStatus::Stocked);
    payload.series = "4.3".to_owned();
    assert!(matches!(
        JigsawPlan::from_wire(&payload, &cfg),
        Err(PlanError::Wire {
            field: "series",
            ..
        })
    ));
}
