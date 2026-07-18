// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Visibility heuristics.
//!
//! Scans Erlang source for the `%% @hidden` and `-doc(hidden)` markers. The
//! classification itself is in `backhopper_core::extract::classify_visibility`.

pub fn detect_visibility_hints(source: &str) -> VisibilityHints {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("%% @hidden")
            || trimmed.starts_with("%%@hidden")
            || trimmed.starts_with("-doc(hidden)")
        {
            return VisibilityHints { hidden: true };
        }
    }
    VisibilityHints::default()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityHints {
    pub hidden: bool,
}
