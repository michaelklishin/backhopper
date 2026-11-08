// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `generate_action` is the pure decision behind `snapshots generate`:
//! for any freshness reading and flag value, `Build` fires exactly
//! when nothing is on disk, and `Refresh` fires only when the flag is
//! set.

use proptest::prelude::*;

use backhopper_cli::commands::auto_generate::{GenerateAction, Presence, generate_action};
use backhopper_core::snapshot::freshness::StaleReason;

fn arb_freshness() -> impl Strategy<Value = Presence> {
    prop_oneof![
        Just(Presence::Current),
        "[a-z0-9]{0,6}"
            .prop_map(|found| Presence::Outdated(StaleReason::ExtractorVersion { found })),
        (0u32..10).prop_map(|found| Presence::Outdated(StaleReason::FormatVersion { found })),
        Just(Presence::Outdated(StaleReason::ExtractionSettings)),
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
        prop_assert_eq!(action == GenerateAction::Refresh, refresh_stale && freshness != Presence::Current);
    }

    #[test]
    fn current_never_refreshes(
        refresh_stale in any::<bool>(),
    ) {
        let action = generate_action(Some(&Presence::Current), refresh_stale);
        prop_assert_eq!(action, GenerateAction::Skip);
    }
}
