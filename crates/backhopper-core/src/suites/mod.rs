// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Test-suite selection.
//!
//! Given a list of modified file paths and a tree of discovered
//! applications, returns the set of CT (Common Test) suite modules
//! that should run, each tagged with the rule(s) that included it.
//!
//! The rules are deliberately generic: no rabbitmq-specific knowledge
//! lives here. Project-specific rules can be expressed as
//! `ExtraRule` entries supplied by the caller.

mod hints;
mod library;
mod matcher;
mod model;
mod plan;
mod rules;
mod scan;

pub use hints::BuildSystem;
pub use library::derive_library_apps;
pub use matcher::{SubstringMatcher, SuiteMatcher, text_references_module};
pub use model::{
    ExtraRule, ExtraRuleTrigger, PlanInput, SuiteInclusionReason, SuitePlan, SuitePlanEntry,
    SuiteRef, SuiteRefSpec,
};
pub use plan::{plan, plan_with_matcher};
