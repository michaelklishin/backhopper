// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `snapshots`-family verb builders.
//!
//! The two diff modes are split into separate builders rather than one
//! flag-overloaded call: `project_diff` diffs one project across two
//! tags, `series_diff` diffs the pins of two series. Both are
//! repo-free and dispatch to `snapshots diff` today.

use std::ffi::OsString;

use backhopper_core::model::names::{ProjectName, SeriesName, TagName};
use backhopper_core::model::snapshot_diff::{CrossSeriesDiffPayload, DiffPayload};

use crate::backend::Backend;
use crate::driver::Backhopper;
use crate::envelope::ExecutedInvocation;
use crate::error::DriverError;
use crate::stdin::StdinPayload;
use crate::verb::Verb;

/// Top-level handle returned by [`Backhopper::snapshots`].
#[derive(Debug)]
pub struct Snapshots<'a, B: Backend> {
    pub(crate) driver: &'a Backhopper<B>,
}

impl<'a, B: Backend> Snapshots<'a, B> {
    /// Diff one project's API between two tags.
    pub fn project_diff(
        &'a self,
        project: impl Into<ProjectName>,
        from: impl Into<TagName>,
        to: impl Into<TagName>,
    ) -> ProjectDiffBuilder<'a, B> {
        ProjectDiffBuilder {
            driver: self.driver,
            project: project.into(),
            from: from.into(),
            to: to.into(),
        }
    }

    /// Diff the dependency pins of two series.
    pub fn series_diff(
        &'a self,
        from_series: impl Into<SeriesName>,
        to_series: impl Into<SeriesName>,
    ) -> SeriesDiffBuilder<'a, B> {
        SeriesDiffBuilder {
            driver: self.driver,
            from_series: from_series.into(),
            to_series: to_series.into(),
        }
    }
}

/// `snapshots diff --project ... --from ... --to ...` builder.
#[derive(Debug)]
#[must_use = "a builder has no effect until .run() is called"]
pub struct ProjectDiffBuilder<'a, B: Backend> {
    driver: &'a Backhopper<B>,
    project: ProjectName,
    from: TagName,
    to: TagName,
}

impl<B: Backend> ProjectDiffBuilder<'_, B> {
    /// Dispatch and return the parsed per-project diff.
    pub fn run(self) -> Result<DiffPayload, DriverError> {
        self.run_with_diagnostics().map(|(diff, _)| diff)
    }

    /// Dispatch and return the parsed diff plus the diagnostic snapshot.
    pub fn run_with_diagnostics(self) -> Result<(DiffPayload, ExecutedInvocation), DriverError> {
        let args = vec![
            OsString::from("--project"),
            OsString::from(self.project.to_string()),
            OsString::from("--from"),
            OsString::from(self.from.to_string()),
            OsString::from("--to"),
            OsString::from(self.to.to_string()),
        ];
        self.driver.dispatch_typed::<DiffPayload>(
            Verb::SnapshotsProjectDiff,
            args,
            StdinPayload::None,
        )
    }
}

/// `snapshots diff --from-series ... --to-series ...` builder.
#[derive(Debug)]
#[must_use = "a builder has no effect until .run() is called"]
pub struct SeriesDiffBuilder<'a, B: Backend> {
    driver: &'a Backhopper<B>,
    from_series: SeriesName,
    to_series: SeriesName,
}

impl<B: Backend> SeriesDiffBuilder<'_, B> {
    /// Dispatch and return the parsed cross-series diff.
    pub fn run(self) -> Result<CrossSeriesDiffPayload, DriverError> {
        self.run_with_diagnostics().map(|(diff, _)| diff)
    }

    /// Dispatch and return the parsed diff plus the diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(CrossSeriesDiffPayload, ExecutedInvocation), DriverError> {
        let args = vec![
            OsString::from("--from-series"),
            OsString::from(self.from_series.to_string()),
            OsString::from("--to-series"),
            OsString::from(self.to_series.to_string()),
        ];
        self.driver.dispatch_typed::<CrossSeriesDiffPayload>(
            Verb::SnapshotsSeriesDiff,
            args,
            StdinPayload::None,
        )
    }
}
