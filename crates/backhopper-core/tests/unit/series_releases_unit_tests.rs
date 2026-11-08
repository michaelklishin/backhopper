// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::jigsaw::{OtpLine, OtpLineError, SeriesReleases, SeriesReleasesError};
use backhopper_core::model::release_version::{HostTag, OtpLineVersion, OtpTag};

use crate::jigsaw_support::{config, rabbitmq, tag};

fn host_tags(values: &[&str]) -> Vec<HostTag> {
    let cfg = config();
    let project = cfg.project(&rabbitmq(&cfg).project).unwrap();
    values
        .iter()
        .map(|value| HostTag::parse(project, tag(value)).unwrap())
        .collect()
}

fn otp_tags(values: &[&str]) -> Vec<OtpTag> {
    let cfg = config();
    let project = cfg.project(&rabbitmq(&cfg).otp_project).unwrap();
    values
        .iter()
        .map(|value| OtpTag::parse(project, tag(value)).unwrap())
        .collect()
}

#[test]
fn releases_are_sorted_ascending_whatever_the_input_order() {
    let releases = SeriesReleases::new(host_tags(&["v4.2.10", "v4.2.2", "v4.2.9"])).unwrap();
    let order: Vec<&str> = releases.iter().map(|r| r.tag().as_str()).collect();
    assert_eq!(order, ["v4.2.2", "v4.2.9", "v4.2.10"]);
}

#[test]
fn the_build_patch_is_the_newest_release() {
    let releases = SeriesReleases::new(host_tags(&["v4.2.1", "v4.2.11", "v4.2.3"])).unwrap();
    assert_eq!(releases.build().tag().as_str(), "v4.2.11");
    assert_eq!(releases.series().to_string(), "4.2");
}

#[test]
fn an_empty_list_is_refused() {
    assert_eq!(
        SeriesReleases::<HostTag>::new(Vec::new()),
        Err(SeriesReleasesError::Empty)
    );
}

#[test]
fn a_release_listed_twice_is_refused() {
    let error = SeriesReleases::new(host_tags(&["v4.2.1", "v4.2.1"])).unwrap_err();
    assert!(matches!(error, SeriesReleasesError::Duplicate(_)));
}

#[test]
fn releases_of_two_series_are_refused() {
    let error = SeriesReleases::new(host_tags(&["v4.2.1", "v4.3.0"])).unwrap_err();
    assert!(matches!(error, SeriesReleasesError::MixedSeries { .. }));
}

#[test]
fn mapping_keeps_the_order_and_checks_it_again() {
    let releases = SeriesReleases::new(host_tags(&["v4.2.3", "v4.2.1"])).unwrap();
    let mapped = releases.clone().map(|release| release).unwrap();
    assert_eq!(mapped, releases);
    let moved = host_tags(&["v4.3.0"]).remove(0);
    assert!(releases.map(|_| moved.clone()).is_err());
}

#[test]
fn an_otp_line_sorts_its_tags() {
    let line = OtpLine::new(
        "27.3".parse::<OtpLineVersion>().unwrap(),
        otp_tags(&["OTP-27.3.4.18", "OTP-27.3", "OTP-27.3.1"]),
    )
    .unwrap();
    let order: Vec<&str> = line.tags().iter().map(|t| t.tag().as_str()).collect();
    assert_eq!(order, ["OTP-27.3", "OTP-27.3.1", "OTP-27.3.4.18"]);
}

#[test]
fn an_otp_line_refuses_a_tag_from_another_line() {
    let error = OtpLine::new(
        "27.3".parse::<OtpLineVersion>().unwrap(),
        otp_tags(&["OTP-27.3", "OTP-27.4"]),
    )
    .unwrap_err();
    assert!(matches!(error, OtpLineError::StrayTag { .. }));
}

#[test]
fn an_otp_line_needs_a_tag() {
    let error = OtpLine::new("27.3".parse::<OtpLineVersion>().unwrap(), Vec::new()).unwrap_err();
    assert_eq!(error, OtpLineError::Empty);
}
