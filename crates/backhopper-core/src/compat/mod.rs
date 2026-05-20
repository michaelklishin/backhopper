// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

pub mod arg_shape;
pub mod call_sites;
pub(crate) mod diff;
pub(crate) mod evaluate;
pub mod otp;
pub mod patch;
pub mod scope;
pub mod source_macros;

pub use otp::is_otp_module;
pub use patch::{
    Analyzed, EvaluationContext, EvaluationFiles, EvaluationInput, Language, Patch, Pinned, Raw,
    Scoped, Sourced, Verdicted, evaluation_state, patch_state,
};
pub use scope::{PinScope, UntrackedTally};
