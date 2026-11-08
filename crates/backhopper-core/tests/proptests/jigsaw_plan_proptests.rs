// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::config::{Config, ConfigFile, ErlangSupport};
use backhopper_core::erlang_mk::DepPin;
use backhopper_core::jigsaw::{
    JigsawPlan, ListedTags, PlanError, ResolvedRelease, SeriesRequest, StoreStatus, TagSourceKind,
    plan_series,
};
use backhopper_core::model::names::{
    CommitSha, DependencyName, DependencyVersion, ExtractorVersion, TagName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::{ErlangMajor, ErlangVersionRange, HostVersion};
use backhopper_core::model::snapshot::VendoredDepSource;
use backhopper_test_support::HOST_CONFIG_TOML;

fn config() -> Config {
    let raw: ConfigFile = toml::from_str(HOST_CONFIG_TOML).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw).unwrap()
}

fn tag(value: &str) -> TagName {
    TagName::new(value).unwrap()
}

fn hex_pin(name: &str, version: &str) -> DepPin {
    DepPin {
        name: DependencyName::new(name).unwrap(),
        version: DependencyVersion::new(version).unwrap(),
        source: VendoredDepSource::Hex,
    }
}

fn draft(patches: &[u32], ra_patch: u32) -> Result<JigsawPlan, PlanError> {
    let cfg = config();
    let host = cfg.hosts.first().unwrap();
    let host_tags: Vec<TagName> = patches.iter().map(|p| tag(&format!("v4.2.{p}"))).collect();
    Ok(plan_series(
        &cfg,
        host,
        &SeriesRequest {
            series: "4.2".parse().unwrap(),
            up_to: None,
            majors: NonEmpty::new(ErlangMajor::try_from(27).unwrap(), []),
        },
        &ListedTags {
            host: host_tags,
            otp: vec![tag("OTP-27.3"), tag("OTP-27.3.1")],
        },
        TagSourceKind::Repository,
        ExtractorVersion::new("9").unwrap(),
        |release| {
            let patch = release.version().to_string();
            let patch: u32 = patch.rsplit('.').next().unwrap().parse().unwrap();
            Ok(ResolvedRelease {
                commit: CommitSha::new(format!("{:040x}", patch + 1)).unwrap(),
                pins: vec![hex_pin("ra", &format!("2.17.{}", ra_patch + patch % 3))],
            })
        },
    )?
    .plan)
}

fn patch_sets() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::btree_set(0u32..30, 1..10).prop_map(|set| set.into_iter().collect())
}

proptest! {
    #[test]
    fn from_wire_of_to_wire_is_the_plan(patches in patch_sets(), ra_patch in 0u32..9) {
        let cfg = config();
        let plan = draft(&patches, ra_patch).unwrap();
        let payload = plan.to_wire(&plan.nothing_stored());
        prop_assert_eq!(JigsawPlan::from_wire(&payload, &cfg).unwrap(), plan);
    }

    #[test]
    fn the_cache_key_does_not_depend_on_the_order_tags_were_listed_in(
        patches in patch_sets(),
        ra_patch in 0u32..9,
    ) {
        let mut reversed = patches.clone();
        reversed.reverse();
        let sorted = draft(&patches, ra_patch).unwrap();
        let shuffled = draft(&reversed, ra_patch).unwrap();
        prop_assert_eq!(sorted.cache_key(), shuffled.cache_key());
    }

    #[test]
    fn the_cache_key_follows_the_required_snapshots(patches in patch_sets()) {
        let one = draft(&patches, 1).unwrap();
        let other = draft(&patches, 2).unwrap();
        prop_assert_ne!(one.cache_key(), other.cache_key());
        let status = StoreStatus::Stocked;
        let from_wire = JigsawPlan::from_wire(&one.to_wire(&status), &config()).unwrap();
        prop_assert_eq!(from_wire.cache_key(), one.cache_key());
    }

    #[test]
    fn range_for_is_the_entry_of_the_greatest_key_not_above_the_release(
        keys in prop::collection::btree_set((0u32..4, 0u32..20), 0..6),
        release in (0u32..4, 0u32..20),
    ) {
        let range = |from: u32| format!("[{from}.0,60.0)").parse::<ErlangVersionRange>().unwrap();
        let version = |(minor, patch): (u32, u32)| format!("4.{minor}.{patch}").parse::<HostVersion>().unwrap();
        let releases: BTreeMap<HostVersion, ErlangVersionRange> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (version(*key), range(index as u32 + 1)))
            .collect();
        let default = range(0);
        let support = ErlangSupport::new(default.clone(), releases.clone());
        let expected = releases
            .iter()
            .rfind(|(key, _)| **key <= version(release))
            .map_or(&default, |(_, range)| range);
        prop_assert_eq!(support.range_for(&version(release)), expected);
    }
}
