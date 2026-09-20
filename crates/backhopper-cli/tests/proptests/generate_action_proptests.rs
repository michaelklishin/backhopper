// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `generate_action` is the pure decision behind `snapshots generate`:
//! for any freshness reading and flag value, `Build` fires exactly
//! when nothing is on disk, and `Refresh` fires only when the flag is
//! set.

use proptest::prelude::*;

use backhopper_cli::commands::auto_generate::{
    ExtractorFreshness, GenerateAction, generate_action,
};

fn arb_freshness() -> impl Strategy<Value = ExtractorFreshness> {
    prop_oneof![
        Just(ExtractorFreshness::Current),
        "[a-z0-9]{0,6}".prop_map(|stored| ExtractorFreshness::Stale { stored }),
        (0u32..10).prop_map(|format_version| ExtractorFreshness::Unversioned { format_version }),
    ]
}

proptest! {
    #[test]
    fn build_fires_exactly_when_nothing_is_present(
        refresh_stale in any::<bool>(),
    ) {
        prop_assert_eq!(generate_action(None, refresh_stale), GenerateAction::Build);
    }

    #[test]
    fn refresh_fires_only_when_the_flag_is_set_and_something_is_present(
        freshness in arb_freshness(),
        refresh_stale in any::<bool>(),
    ) {
        let action = generate_action(Some(&freshness), refresh_stale);
        prop_assert_ne!(action, GenerateAction::Build);
        prop_assert_eq!(action == GenerateAction::Refresh, refresh_stale && freshness != ExtractorFreshness::Current);
    }

    #[test]
    fn current_never_refreshes(
        refresh_stale in any::<bool>(),
    ) {
        let action = generate_action(Some(&ExtractorFreshness::Current), refresh_stale);
        prop_assert_eq!(action, GenerateAction::Skip);
    }
}
