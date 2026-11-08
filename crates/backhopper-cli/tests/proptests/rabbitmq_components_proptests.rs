// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Property tests for RabbitMQ's branch and tag conventions. Targets:
//!
//!  * `series_name_for_branch` and `version_to_tag` are total functions
//!  * `version_to_tag` is idempotent on already-prefixed input

use proptest::prelude::*;

use backhopper_cli::commands::rabbitmq_components::{series_name_for_branch, version_to_tag};

proptest! {
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
}
