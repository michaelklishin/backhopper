// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::config::{Config, ConfigFile, Project};
use backhopper_core::model::names::{ApplicationName, ProjectName, TagName};
use backhopper_core::snapshot::freshness::extraction_settings_digest;

const CONFIG: &str = r#"
config_version = 1

[[project]]
name = "rabbitmq-server"
git_url = "https://github.com/rabbitmq/rabbitmq-server.git"
layout = "multi_app"
app_roots = ["deps/*"]
"#;

fn project() -> Project {
    let raw: ConfigFile = toml::from_str(CONFIG).unwrap();
    let cfg = Config::from_raw("/tmp/backhopper.toml".into(), raw).unwrap();
    cfg.project(&ProjectName::new("rabbitmq-server").unwrap())
        .unwrap()
        .clone()
}

fn arb_apps() -> impl Strategy<Value = Vec<ApplicationName>> {
    prop::collection::vec("[a-z][a-z0-9_]{0,8}", 0..6).prop_map(|names| {
        names
            .into_iter()
            .map(|n| ApplicationName::new(n).unwrap())
            .collect()
    })
}

proptest! {
    #[test]
    fn list_order_and_repeats_never_change_the_digest(
        apps in arb_apps(),
        seed in any::<u64>(),
    ) {
        let mut ordered = project();
        ordered.exclude_apps = apps.clone();
        let mut shuffled = project();
        let mut rotated = apps;
        if !rotated.is_empty() {
            let by = (seed as usize) % rotated.len();
            rotated.rotate_left(by);
            rotated.push(rotated[0].clone());
        }
        shuffled.exclude_apps = rotated;
        prop_assert_eq!(
            extraction_settings_digest(&ordered),
            extraction_settings_digest(&shuffled)
        );
    }

    #[test]
    fn fields_that_do_not_reach_extraction_never_change_the_digest(
        min_tag in prop::option::of("v[0-9]\\.[0-9]\\.[0-9]"),
        prefix in "[a-z-]{0,6}",
        markers in prop::collection::vec("-[a-z]{1,4}", 0..3),
    ) {
        let mut other = project();
        other.min_tag = min_tag.map(|tag| TagName::new(tag).unwrap());
        other.tag_prefix = prefix;
        other.exclude_tag_markers = markers;
        prop_assert_eq!(
            extraction_settings_digest(&other),
            extraction_settings_digest(&project())
        );
    }

    #[test]
    fn a_different_exclusion_set_changes_the_digest(
        first in arb_apps(),
        second in arb_apps(),
    ) {
        let as_set = |apps: &[ApplicationName]| {
            let mut names: Vec<_> = apps.iter().map(ToString::to_string).collect();
            names.sort();
            names.dedup();
            names
        };
        prop_assume!(as_set(&first) != as_set(&second));
        let mut a = project();
        a.exclude_apps = first;
        let mut b = project();
        b.exclude_apps = second;
        prop_assert_ne!(extraction_settings_digest(&a), extraction_settings_digest(&b));
    }
}
