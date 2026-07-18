// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The verdict fingerprint: a version-independent join key tying a
//! verdict to a later-observed build outcome.
//!
//! The verdict cache hashes everything an evaluation reads, the crate
//! and schema versions included, so one release never serves another's
//! cached verdict. A measurement join needs the same key for the same
//! `(patch, target, pins)` across releases. The fingerprint is derived
//! in `backhopper-cache` from the content-key inputs minus the version
//! fields; this module owns the type and its version stamp.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Bumped only when the fingerprint's inputs change, never on an
/// ordinary release, so a release that leaves the inputs alone keeps
/// prior rounds joinable.
pub const FINGERPRINT_VERSION: u32 = 1;

/// A verdict's stable identity for measurement: equal fingerprints
/// mean the same patch against the same target and pins, whatever
/// backhopper version produced either side.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct VerdictFingerprint(String);

impl VerdictFingerprint {
    /// Wrap a precomputed hex digest; the derivation is in
    /// `backhopper-cache`.
    #[must_use]
    pub fn new(hex: impl Into<String>) -> Self {
        Self(hex.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for VerdictFingerprint {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VerdictFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for VerdictFingerprint {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}
