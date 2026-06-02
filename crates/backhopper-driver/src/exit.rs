// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Exit-code routing.
//!
//! `backhopper` exits with `0`, `2`, `3`, or `4` for verdict
//! outcomes (compatible, incompatible, requires-adaptation,
//! inapplicable), `1` for grep-style negative queries, and a small
//! family of `sysexits.h` codes for genuine errors. The driver
//! routes the verdict and grep-style codes through the typed
//! envelope path; everything else surfaces as `DriverError::ToolError`.

use serde::{Deserialize, Serialize};

/// Classification of a child process's exit code.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExitClass {
    /// Verdict outcomes: 0 (compatible), 2 (incompatible),
    /// 3 (requires-adaptation), 4 (inapplicable).
    VerdictOutcome,
    /// Grep-style negative: code 1, parsed payload is meaningful.
    SuccessShapedQuery,
    /// 64: invalid CLI args; the driver should never produce this.
    UsageError,
    /// 65: malformed snapshot, malformed patch, or schema mismatch.
    DataError,
    /// 66: config or snapshot file not found.
    NoInput,
    /// 74: git or filesystem error.
    IoError,
    /// Anything outside the documented set.
    Unknown(i32),
}

impl ExitClass {
    /// Classify an integer exit code.
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 | 2 | 3 | 4 => Self::VerdictOutcome,
            1 => Self::SuccessShapedQuery,
            64 => Self::UsageError,
            65 => Self::DataError,
            66 => Self::NoInput,
            74 => Self::IoError,
            other => Self::Unknown(other),
        }
    }

    /// True for outcomes that carry a parsed payload.
    ///
    /// The driver matches this convention: success-shaped exits flow
    /// through the typed envelope path, everything else turns into
    /// `DriverError::ToolError`.
    #[must_use]
    pub const fn is_success_shaped(self) -> bool {
        matches!(self, Self::VerdictOutcome | Self::SuccessShapedQuery)
    }

    /// True for the verdict family (0, 2, 3, 4) specifically.
    #[must_use]
    pub const fn is_verdict(self) -> bool {
        matches!(self, Self::VerdictOutcome)
    }
}
