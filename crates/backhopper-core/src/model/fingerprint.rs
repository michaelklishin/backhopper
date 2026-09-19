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

use crate::errors::NameError;

/// Bumped only when the fingerprint's inputs change, never on an
/// ordinary release, so a release that leaves the inputs alone keeps
/// prior rounds joinable.
pub const FINGERPRINT_VERSION: u32 = 1;

/// Bytes in the truncated BLAKE3 digest `VerdictFingerprint` wraps;
/// `backhopper-cache::cache_io` keeps the same `DIGEST_LEN` beside its
/// hex-character `KEY_HASH_LEN`.
const DIGEST_LEN: usize = 16;

/// A verdict's stable identity for measurement: equal fingerprints
/// mean the same patch against the same target and pins, whatever
/// backhopper version produced either side. Thirty-two lowercase hex
/// characters: a truncated BLAKE3 digest, formatted here so
/// `backhopper-core` takes no dependency on `blake3`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(try_from = "String", into = "String")]
pub struct VerdictFingerprint(String);

impl VerdictFingerprint {
    /// Format a precomputed digest. Infallible: any `[u8; 16]` formats
    /// to exactly 32 lowercase hex characters, which is what
    /// `TryFrom<String>` accepts.
    #[must_use]
    pub fn from_digest(digest: [u8; DIGEST_LEN]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut hex = String::with_capacity(DIGEST_LEN * 2);
        for byte in digest {
            hex.push(HEX[usize::from(byte >> 4)] as char);
            hex.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        Self(hex)
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

impl TryFrom<String> for VerdictFingerprint {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, NameError> {
        let valid = value.len() == DIGEST_LEN * 2
            && value
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
        if !valid {
            return Err(NameError::InvalidVerdictFingerprint { value });
        }
        Ok(Self(value))
    }
}

impl From<VerdictFingerprint> for String {
    fn from(f: VerdictFingerprint) -> Self {
        f.0
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
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        Self::try_from(s.to_owned())
    }
}
