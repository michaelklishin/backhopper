// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The edoc half of module visibility. `-moduledoc false.` is the other
//! half and is read as an attribute; the classification itself is in
//! `backhopper_core::extract::classify_visibility`.

/// True when a `%` comment line before the `-module` attribute (at the
/// 1-based `module_line`) carries edoc's `@hidden` tag. edoc reads
/// module-level tags from that header only: an `@hidden` further down
/// documents one function.
pub fn module_header_hides(source: &str, module_line: usize) -> bool {
    source
        .lines()
        .take(module_line.saturating_sub(1))
        .filter_map(|line| line.trim_start().strip_prefix('%'))
        .any(|comment| {
            comment
                .trim_start_matches('%')
                .trim_start()
                .starts_with("@hidden")
        })
}
