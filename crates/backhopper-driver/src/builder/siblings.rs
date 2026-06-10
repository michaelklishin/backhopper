// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `siblings`-family verb builders.
//!
//! `siblings doctor` has exactly one required argument, the series,
//! so requiredness is enforced by the constructor instead of a
//! type-state marker: there is no way to build the builder without
//! it.

use std::ffi::OsString;
use std::path::PathBuf;

use backhopper_core::model::names::{GitRef, SeriesName};
use backhopper_core::model::sibling_drift::SiblingDoctorReport;

use crate::backend::Backend;
use crate::driver::Backhopper;
use crate::envelope::ExecutedInvocation;
use crate::error::DriverError;
use crate::stdin::StdinPayload;
use crate::verb::{Verb, VerbId};

/// Top-level handle returned by [`Backhopper::siblings`].
#[derive(Debug)]
pub struct Siblings<'a, B: Backend> {
    pub(crate) driver: &'a Backhopper<B>,
}

impl<'a, B: Backend> Siblings<'a, B> {
    /// Build a `siblings doctor` invocation for `series`.
    pub fn doctor(&'a self, series: &SeriesName) -> SiblingsDoctorBuilder<'a, B> {
        SiblingsDoctorBuilder {
            driver: self.driver,
            series: series.clone(),
            source_branches: Vec::new(),
            target_branch: None,
            since: None,
            top: None,
            vocabulary_file_path: None,
            explain: false,
            no_cache: false,
        }
    }
}

/// `siblings doctor` builder. Every setter is optional; the CLI's
/// defaults apply to anything left unset.
#[derive(Debug)]
#[must_use = "a builder has no effect until .run() is called"]
pub struct SiblingsDoctorBuilder<'a, B: Backend> {
    driver: &'a Backhopper<B>,
    series: SeriesName,
    source_branches: Vec<GitRef>,
    target_branch: Option<GitRef>,
    since: Option<String>,
    top: Option<u32>,
    vocabulary_file_path: Option<PathBuf>,
    explain: bool,
    no_cache: bool,
}

impl<B: Backend> SiblingsDoctorBuilder<'_, B> {
    /// Add one `--source-branch`. Repeatable; the CLI defaults to
    /// `main` when none is given.
    pub fn source_branch(mut self, branch: GitRef) -> Self {
        self.source_branches.push(branch);
        self
    }

    /// Set `--target-branch`, overriding the series' self-pin.
    pub fn target_branch(mut self, branch: GitRef) -> Self {
        self.target_branch = Some(branch);
        self
    }

    /// Set `--since` (a tag name or commit SHA prefix).
    pub fn since(mut self, since: impl Into<String>) -> Self {
        self.since = Some(since.into());
        self
    }

    /// Set `--top`.
    pub fn top(mut self, top: u32) -> Self {
        self.top = Some(top);
        self
    }

    /// Set `--vocabulary-file-path`.
    pub fn vocabulary_file_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.vocabulary_file_path = Some(path.into());
        self
    }

    /// Pass `--explain` so every candidate carries its score
    /// breakdown.
    pub fn explain(mut self) -> Self {
        self.explain = true;
        self
    }

    /// Pass `--no-cache`.
    pub fn no_cache(mut self) -> Self {
        self.no_cache = true;
        self
    }

    /// Run `siblings doctor` and parse the report payload.
    pub fn run(self) -> Result<(SiblingDoctorReport, ExecutedInvocation), DriverError> {
        let mut args: Vec<OsString> = Vec::new();
        args.push("--series".into());
        args.push(self.series.as_str().into());
        for branch in &self.source_branches {
            args.push("--source-branch".into());
            args.push(branch.as_str().into());
        }
        if let Some(branch) = &self.target_branch {
            args.push("--target-branch".into());
            args.push(branch.as_str().into());
        }
        if let Some(since) = &self.since {
            args.push("--since".into());
            args.push(since.into());
        }
        if let Some(top) = self.top {
            args.push("--top".into());
            args.push(top.to_string().into());
        }
        if let Some(path) = &self.vocabulary_file_path {
            args.push("--vocabulary-file-path".into());
            args.push(path.into());
        }
        if self.explain {
            args.push("--explain".into());
        }
        if self.no_cache {
            args.push("--no-cache".into());
        }
        let envelope = self.driver.invoke::<SiblingDoctorReport, OsString>(
            VerbId::Known(Verb::SiblingsDoctor),
            &args,
            StdinPayload::None,
        )?;
        let (_, _, data, _, _, executed) = envelope.into_parts();
        Ok((data, executed))
    }
}
