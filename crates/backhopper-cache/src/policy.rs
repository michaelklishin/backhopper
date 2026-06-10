// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Process-wide cache on/off resolution.
//!
//! Precedence, strongest first: an explicit `--no-cache`-style flag,
//! `BACKHOPPER_NO_CACHE`, `BACKHOPPER_FORCE_CACHE`, then the build
//! profile default (debug builds bypass the cache so iterating on
//! scoring or walk code never serves yesterday's answers).

use std::env;

/// Disables caching for the process when set.
pub const NO_CACHE_ENV: &str = "BACKHOPPER_NO_CACHE";

/// Re-enables caching in debug builds when set.
pub const FORCE_CACHE_ENV: &str = "BACKHOPPER_FORCE_CACHE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    Enabled,
    Disabled,
}

impl CacheMode {
    /// Resolve the effective mode for this process from the real
    /// environment.
    #[must_use]
    pub fn resolve(cli_no_cache: bool) -> Self {
        Self::resolve_with(cli_no_cache, |name| env::var(name).ok())
    }

    /// Pure resolution against an injected environment lookup. The
    /// build-profile default comes from `cfg!(debug_assertions)`.
    #[must_use]
    pub fn resolve_with(cli_no_cache: bool, env_lookup: impl Fn(&str) -> Option<String>) -> Self {
        if cli_no_cache || flag_set(env_lookup(NO_CACHE_ENV)) {
            return Self::Disabled;
        }
        if flag_set(env_lookup(FORCE_CACHE_ENV)) {
            return Self::Enabled;
        }
        if cfg!(debug_assertions) {
            Self::Disabled
        } else {
            Self::Enabled
        }
    }

    #[must_use]
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

// set means non-empty and not "0", so `BACKHOPPER_NO_CACHE=0` reads as off
fn flag_set(value: Option<String>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}
