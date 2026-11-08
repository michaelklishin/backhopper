// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::release_version::{HostSeries, ReleaseVersion};
use backhopper_core::versions::version_cmp;
use proptest::prelude::*;

fn release() -> impl Strategy<Value = String> {
    proptest::collection::vec(0u32..400, 2..=6).prop_map(|segments| {
        segments
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    })
}

proptest! {
    #[test]
    fn display_then_parse_is_the_identity(value in release()) {
        let version = ReleaseVersion::parse(&value).unwrap();
        prop_assert_eq!(version.to_string(), value);
    }

    #[test]
    fn ordering_is_the_reverse_of_version_cmp(a in release(), b in release()) {
        let (left, right) = (ReleaseVersion::parse(&a).unwrap(), ReleaseVersion::parse(&b).unwrap());
        prop_assert_eq!(left.cmp(&right), version_cmp(&b, &a));
    }

    #[test]
    fn sorting_versions_matches_sorting_tags_with_version_cmp_reversed(
        values in proptest::collection::vec(release(), 1..12)
    ) {
        let mut versions: Vec<_> = values.iter().map(|v| ReleaseVersion::parse(v).unwrap()).collect();
        versions.sort();
        let mut tags = values.clone();
        tags.sort_by(|a, b| version_cmp(a, b).reverse());
        let by_version: Vec<_> = versions.iter().map(ToString::to_string).collect();
        prop_assert_eq!(by_version, tags);
    }

    #[test]
    fn series_display_then_parse_is_the_identity(major in 0u32..100, minor in 0u32..100) {
        let text = format!("{major}.{minor}");
        prop_assert_eq!(text.parse::<HostSeries>().unwrap().to_string(), text);
    }
}
