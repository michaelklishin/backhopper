// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{
    ExtractorVersion, HostName, PlanCacheKey, SeriesTagPattern, StoreDigest,
};
use backhopper_core::model::release_version::HostSeries;

fn series(value: &str) -> HostSeries {
    value.parse().unwrap()
}

#[test]
fn a_series_tag_pattern_has_exactly_one_placeholder() {
    for valid in ["v{series}.*", "{series}.*", "rabbitmq-v{series}.?"] {
        assert!(SeriesTagPattern::new(valid).is_ok(), "{valid}");
    }
    for invalid in [
        "v*",
        "v{series}.{series}",
        "v{series}{",
        "{major}.*",
        "",
        "v{series}/*",
    ] {
        assert!(SeriesTagPattern::new(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_series_tag_pattern_becomes_a_tag_glob_for_a_series() {
    let pattern = SeriesTagPattern::new("v{series}.*").unwrap();
    assert_eq!(pattern.for_series(series("4.2")).as_str(), "v4.2.*");
}

#[test]
fn the_glob_for_a_series_does_not_match_a_longer_minor() {
    let glob = SeriesTagPattern::new("v{series}.*")
        .unwrap()
        .for_series(series("4.2"));
    let matches = |tag: &str| glob.matches(&tag.parse().unwrap());
    assert!(matches("v4.2.8"));
    assert!(!matches("v4.20.0"));
}

#[test]
fn a_host_name_has_the_charset_of_a_project_name() {
    assert!(HostName::new("rabbitmq").is_ok());
    assert!(HostName::new("my-host_2").is_ok());
    for invalid in ["", "Rabbit", "1host", "a/b"] {
        assert!(HostName::new(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn an_extractor_version_is_a_short_dotted_token() {
    assert!(ExtractorVersion::new("9").is_ok());
    assert!(ExtractorVersion::new("10.1").is_ok());
    for invalid in ["", "9 ", "a-b", "12345678901234567"] {
        assert!(ExtractorVersion::new(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn a_plan_cache_key_is_composed_from_its_parts() {
    let key = PlanCacheKey::compose(
        &HostName::new("rabbitmq").unwrap(),
        series("4.2"),
        5,
        &ExtractorVersion::new("9").unwrap(),
        [0x3f, 0x1a, 0x9c, 0x0d, 0x27, 0xe4, 0xb6, 0xa1],
    );
    assert_eq!(key.as_str(), "jigsaw-rabbitmq-4.2-f5-e9-3f1a9c0d27e4b6a1");
    assert_eq!(key.restore_prefix(), "jigsaw-rabbitmq-4.2-f5-e9-");
    assert_eq!(PlanCacheKey::new(key.as_str()).unwrap(), key);
}

#[test]
fn a_plan_cache_key_refuses_a_string_of_another_shape() {
    for invalid in [
        "rabbitmq-4.2-f5-e9-3f1a9c0d27e4b6a1",
        "jigsaw-rabbitmq-4.2-f5-e9-3f1a9c0d27e4b6a",
        "jigsaw-rabbitmq-4.2-f5-e9-3F1A9C0D27E4B6A1",
        "jigsaw-rabbitmq-4.2-f5-e93f1a9c0d27e4b6a1",
        "jigsaw-Rabbit-4.2-f5-e9-3f1a9c0d27e4b6a1",
    ] {
        assert!(PlanCacheKey::new(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_store_digest_is_sixty_four_lowercase_hex_characters() {
    let digest = StoreDigest::from_digest([0xab; 32]);
    assert_eq!(digest.as_str(), "ab".repeat(32));
    assert!(StoreDigest::new("ab".repeat(31)).is_err());
    assert!(StoreDigest::new("AB".repeat(32)).is_err());
}
