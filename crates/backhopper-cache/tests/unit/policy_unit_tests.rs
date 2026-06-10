// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cache::{CacheMode, FORCE_CACHE_ENV, NO_CACHE_ENV};

// the test profile compiles with debug assertions, so the build-default branch is Disabled
const BUILD_DEFAULT: CacheMode = if cfg!(debug_assertions) {
    CacheMode::Disabled
} else {
    CacheMode::Enabled
};

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| {
        owned
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

#[test]
fn build_profile_default_applies_when_nothing_is_set() {
    let mode = CacheMode::resolve_with(false, env_of(&[]));
    assert_eq!(mode, BUILD_DEFAULT);
}

#[test]
fn cli_no_cache_always_wins() {
    let mode = CacheMode::resolve_with(true, env_of(&[(FORCE_CACHE_ENV, "1")]));
    assert_eq!(mode, CacheMode::Disabled);
}

#[test]
fn no_cache_env_beats_force_cache_env() {
    let mode = CacheMode::resolve_with(
        false,
        env_of(&[(NO_CACHE_ENV, "1"), (FORCE_CACHE_ENV, "1")]),
    );
    assert_eq!(mode, CacheMode::Disabled);
}

#[test]
fn force_cache_env_enables_in_any_build() {
    let mode = CacheMode::resolve_with(false, env_of(&[(FORCE_CACHE_ENV, "1")]));
    assert_eq!(mode, CacheMode::Enabled);
}

#[test]
fn zero_and_empty_values_read_as_unset() {
    let mode =
        CacheMode::resolve_with(false, env_of(&[(NO_CACHE_ENV, "0"), (FORCE_CACHE_ENV, "")]));
    assert_eq!(mode, BUILD_DEFAULT);
}

#[test]
fn is_enabled_matches_the_variant() {
    assert!(CacheMode::Enabled.is_enabled());
    assert!(!CacheMode::Disabled.is_enabled());
}
