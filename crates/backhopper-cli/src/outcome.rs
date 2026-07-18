// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The value a command handler returns instead of a bare `i32`, so the
//! "only success or partial-success" invariant is a compile-time fact.

use bel7_cli::PARTIAL_SUCCESS_I32;

/// The outcome of a command: every pin clean, or at least one needs
/// attention. There is no third exit code a handler may produce; error
/// paths flow through `CliError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Success,
    PartialSuccess,
}

impl CommandOutcome {
    /// The process exit code and the value carried in the JSON envelope.
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::PartialSuccess => PARTIAL_SUCCESS_I32,
        }
    }

    /// `Success` when `success`, `PartialSuccess` otherwise: the common
    /// shape of a handler that ends on a boolean.
    #[must_use]
    pub fn from_success(success: bool) -> Self {
        if success {
            Self::Success
        } else {
            Self::PartialSuccess
        }
    }
}
