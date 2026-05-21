// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{TagGlob, TagName};
use proptest::prelude::*;

fn safe_tag() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9-]{0,16}(\\.[0-9]{1,3}){0,3}"
        .prop_filter("non-empty", |s: &String| !s.is_empty())
}

proptest! {
    #[test]
    fn star_matches_every_tag(tag_str in safe_tag()) {
        let tag = match TagName::new(tag_str.clone()) {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };
        let g = TagGlob::new("*").unwrap();
        prop_assert!(g.matches(&tag));
    }

    #[test]
    fn literal_matches_self_only(tag_str in safe_tag()) {
        let tag = match TagName::new(tag_str.clone()) {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };
        let g = match TagGlob::new(tag_str.clone()) {
            Ok(g) => g,
            Err(_) => return Ok(()),
        };
        prop_assert!(g.matches(&tag));
    }

    #[test]
    fn prefix_star_matches_only_with_prefix(
        prefix in "[A-Za-z]{1,8}-",
        suffix in "[A-Za-z0-9]{1,8}",
        other in "[A-Za-z]{1,8}",
    ) {
        let glob_str = format!("{prefix}*");
        let g = match TagGlob::new(glob_str.clone()) {
            Ok(g) => g,
            Err(_) => return Ok(()),
        };
        let matching_tag = format!("{prefix}{suffix}");
        let nonmatching_tag = format!("{other}-{suffix}");
        if let Ok(t) = TagName::new(matching_tag) {
            prop_assert!(g.matches(&t));
        }
        if other != prefix.trim_end_matches('-')
            && let Ok(t) = TagName::new(nonmatching_tag)
        {
            prop_assert!(!g.matches(&t));
        }
    }
}
