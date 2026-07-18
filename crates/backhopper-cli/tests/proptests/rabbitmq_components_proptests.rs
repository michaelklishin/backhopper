// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Property tests for the RabbitMQ components.mk parser. Targets:
//!
//!  * arbitrary text never panics
//!  * a synthesized `dep_NAME = hex VERSION` line always parses to a pin
//!  * `series_name_for_branch` and `version_to_tag` are total functions
//!  * `version_to_tag` is idempotent on already-prefixed input

use proptest::prelude::*;

use backhopper_cli::commands::rabbitmq_components::{
    DepSource, parse_components_mk, series_name_for_branch, version_to_tag,
};

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
        let line = format!("dep_{name} = hex {version}\n");
        let pins = parse_components_mk(&line);
        // _commit, _branch, and _repo suffixes are ignored, so they would not produce a pin
        if !(name.ends_with("_commit") || name.ends_with("_branch") || name.ends_with("_repo")) {
            prop_assert_eq!(pins.len(), 1);
            prop_assert_eq!(&pins[0].name, &name);
            prop_assert_eq!(&pins[0].version, &version);
            prop_assert_eq!(pins[0].source, DepSource::Hex);
        }
    }

    #[test]
    fn git_url_tag_line_records_tag_only(
        name in "[a-z][a-z0-9_]{0,12}",
        tag in "v[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
    ) {
        let line = format!(
            "dep_{name} = git https://example.com/repo.git {tag}\n"
        );
        let pins = parse_components_mk(&line);
        if !(name.ends_with("_commit") || name.ends_with("_branch") || name.ends_with("_repo")) {
            prop_assert_eq!(pins.len(), 1);
            prop_assert_eq!(&pins[0].version, &tag);
            prop_assert_eq!(pins[0].source, DepSource::Git);
        }
    }

    #[test]
    fn series_name_always_starts_with_rabbitmq_prefix(branch in "[a-z0-9./_-]{1,32}") {
        let s = series_name_for_branch(&branch);
        prop_assert!(s.starts_with("rabbitmq-"), "got {s} for branch {branch}");
    }

    #[test]
    fn version_to_tag_is_idempotent_when_prefix_already_present(
        prefix in "v|",
        version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
    ) {
        let once = version_to_tag(&version, &prefix);
        let twice = version_to_tag(&once, &prefix);
        prop_assert_eq!(once, twice);
    }

    #[test]
    fn comment_lines_are_always_skipped(name in "[a-z][a-z0-9_]{0,8}", v in "[0-9]{1,3}") {
        let body = format!("# dep_{name} = hex {v}\n");
        let pins = parse_components_mk(&body);
        prop_assert!(pins.is_empty(), "comment leaked into output: {pins:?}");
    }
}
