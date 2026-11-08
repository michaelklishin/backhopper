// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::erlang_mk::{DepSource, parse_components_mk};

fn is_override_name(name: &str) -> bool {
    name.ends_with("_commit") || name.ends_with("_branch") || name.ends_with("_repo")
}

proptest! {
    #[test]
    fn arbitrary_input_never_panics(text in "[\\x20-\\x7e\\n]{0,512}") {
        let _ = parse_components_mk(&text);
    }

    #[test]
    fn well_formed_hex_line_round_trips(
        name in "[a-z][a-z0-9_]{0,12}",
        version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
    ) {
        let pins = parse_components_mk(&format!("dep_{name} = hex {version}\n"));
        if !is_override_name(&name) {
            prop_assert_eq!(pins.len(), 1);
            prop_assert_eq!(pins[0].name.as_str(), name.as_str());
            prop_assert_eq!(pins[0].version.as_str(), version.as_str());
            prop_assert_eq!(pins[0].source, DepSource::Hex);
        }
    }

    #[test]
    fn git_url_tag_line_records_tag_only(
        name in "[a-z][a-z0-9_]{0,12}",
        tag in "v[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
    ) {
        let line = format!("dep_{name} = git https://example.com/repo.git {tag}\n");
        let pins = parse_components_mk(&line);
        if !is_override_name(&name) {
            prop_assert_eq!(pins.len(), 1);
            prop_assert_eq!(pins[0].version.as_str(), tag.as_str());
            prop_assert_eq!(pins[0].source, DepSource::Git);
        }
    }

    #[test]
    fn comment_lines_are_always_skipped(name in "[a-z][a-z0-9_]{0,8}", v in "[0-9]{1,3}") {
        let pins = parse_components_mk(&format!("# dep_{name} = hex {v}\n"));
        prop_assert!(pins.is_empty(), "comment leaked into output: {pins:?}");
    }
}
