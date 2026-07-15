// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

pub mod added_file;
pub mod added_lines;
pub mod arg_shape;
pub mod behaviour_callback_resolve;
pub mod call_sites;
pub mod define_resolve;
pub(crate) mod diff;
pub(crate) mod evaluate;
pub mod exported_type_resolve;
pub mod indirect_calls;
pub mod local_call_resolve;
pub mod otp;
pub mod patch;
pub(crate) mod patch_facts;
pub mod qualified_call_resolve;
pub mod routing;
pub mod scope;
pub mod source_attributes;
pub mod source_macros;
pub mod target_tree;
pub mod target_tree_index;
pub mod test_suite;

pub use evaluate::classify_hunks_against_target;
pub use otp::is_otp_module;
pub use patch::{
    Analyzed, EvaluationContext, EvaluationFiles, EvaluationInput, Language, Patch, Pinned, Raw,
    Scoped, Sourced, Verdicted, evaluation_state, patch_state,
};
pub use routing::{
    PathRouting, RoutedPinVerdict, classify_paths_for_pin, project_owns_path, route_pin_verdict,
};
pub use scope::{PinScope, UntrackedTally};
pub use target_tree::{TargetPathClassification, TouchedPathQuery, classify_path, normalise};
