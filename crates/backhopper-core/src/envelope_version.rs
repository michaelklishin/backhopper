// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Envelope schema version constants. Lives outside the `schemars`-gated
//! `schema` module so consumers that do not pull in the JSON Schema
//! machinery can still read the version numbers.

/// The most recent envelope schema version this build knows about.
/// Bumps on every wire-format change; a fresh `vN.json` is regenerated
/// in lockstep via `cargo xtask gen-schema --bless`.
pub const CURRENT_SCHEMA_VERSION: u32 = 11;

/// Lowest version this build embeds. Old versions stay embedded so
/// `schema show <N>` can answer for any version the binary ever
/// shipped.
pub const MIN_EMBEDDED_VERSION: u32 = 1;

/// Versions this build embeds, in ascending order.
#[must_use]
pub fn embedded_versions() -> Vec<u32> {
    (MIN_EMBEDDED_VERSION..=CURRENT_SCHEMA_VERSION).collect()
}
