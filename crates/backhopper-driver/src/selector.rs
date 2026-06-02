// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! "Where" a check runs: against a series, or against a single pin.

use backhopper_core::model::names::{ProjectName, SeriesName, TagName};

/// Either a named series or a single `(project, tag)` pin.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinSelector {
    /// `--series NAME`.
    Series(SeriesName),
    /// `--project NAME --tag TAG`.
    Pin {
        /// Configured project name.
        project: ProjectName,
        /// Pinned tag.
        tag: TagName,
    },
}

impl PinSelector {
    /// Build a series selector.
    pub fn series(name: impl Into<SeriesName>) -> Self {
        Self::Series(name.into())
    }

    /// Build a single-pin selector.
    pub fn pin(project: impl Into<ProjectName>, tag: impl Into<TagName>) -> Self {
        Self::Pin {
            project: project.into(),
            tag: tag.into(),
        }
    }
}

impl From<SeriesName> for PinSelector {
    fn from(s: SeriesName) -> Self {
        Self::Series(s)
    }
}

impl From<(ProjectName, TagName)> for PinSelector {
    fn from((project, tag): (ProjectName, TagName)) -> Self {
        Self::Pin { project, tag }
    }
}
