// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Type-state builders for the verbs the driver wraps.

pub mod check;
pub mod siblings;
pub mod snapshots;
pub mod state;
pub mod suites;

pub use check::{
    Check, CheckBatchBuilder, CheckCommitBuilder, CheckMergeBuilder, CheckPatchBuilder,
    CheckRangeBuilder, ExplainFormat, PatchSource, SeriesEvaluation,
};
pub use siblings::{Siblings, SiblingsDoctorBuilder};
pub use snapshots::{ProjectDiffBuilder, SeriesDiffBuilder, Snapshots};
pub use suites::{Suites, SuitesPlanBuilder};
