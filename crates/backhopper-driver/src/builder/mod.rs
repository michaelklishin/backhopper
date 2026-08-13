// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Type-state builders for the verbs the driver wraps.

pub mod check;
pub(crate) mod list_input;
pub mod snapshots;
pub mod state;
pub mod suites;

pub use check::{
    Check, CheckBatchBuilder, CheckCommitBuilder, CheckMergeBuilder, CheckPatchBuilder,
    CheckPositionalBuilder, CheckRangeBuilder, CommitKind, MergeKind, PatchSource, PositionalKind,
    RangeKind,
};
pub use snapshots::{ProjectDiffBuilder, SeriesDiffBuilder, Snapshots};
pub use suites::{Suites, SuitesPlanBuilder};
