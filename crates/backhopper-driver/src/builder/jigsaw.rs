// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `jigsaw`-family verb builders.
//!
//! `jigsaw plan` needs a host and a series. `jigsaw check` needs a plan
//! source and at least one plugin directory. Each requirement is a type
//! parameter, so `run` exists only once all of them are filled.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use backhopper_core::jigsaw::{JigsawCheckPayload, JigsawPlanPayload};
use backhopper_core::model::names::{HostName, TagName};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::{ErlangMajor, HostSeries};

use crate::backend::Backend;
use crate::builder::state::{
    HostState, NoHost, NoPlugins, NoSeries, NoTarget, PluginsState, SeriesState, TargetState,
    WithCheckTarget, WithHost, WithPlugins, WithSeries,
};
use crate::driver::Backhopper;
use crate::envelope::ExecutedInvocation;
use crate::error::DriverError;
use crate::stdin::StdinPayload;
use crate::verb::Verb;

/// Top-level handle returned by [`Backhopper::jigsaw`].
#[derive(Debug)]
pub struct Jigsaw<'a, B: Backend> {
    pub(crate) driver: &'a Backhopper<B>,
}

impl<'a, B: Backend> Jigsaw<'a, B> {
    /// Build a `jigsaw plan` invocation.
    pub fn plan(&'a self) -> JigsawPlanBuilder<'a, B, NoHost, NoSeries> {
        JigsawPlanBuilder {
            driver: self.driver,
            host: NoHost,
            series: NoSeries,
            options: PlanOptions::default(),
            plan_file_path: None,
        }
    }

    /// Build a `jigsaw check` invocation.
    pub fn check(&'a self) -> JigsawCheckBuilder<'a, B, NoTarget, NoPlugins> {
        JigsawCheckBuilder {
            driver: self.driver,
            target: NoTarget,
            plugins: NoPlugins,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PlanOptions {
    up_to: Option<TagName>,
    erlang_majors: Vec<ErlangMajor>,
}

/// What `jigsaw plan` is asked for, in the form `jigsaw check` accepts
/// in place of a plan file.
#[derive(Debug, Clone)]
pub struct PlanArguments {
    host: HostName,
    series: HostSeries,
    options: PlanOptions,
}

impl PlanArguments {
    /// The plan of `series` for the configured `host`, with the host's
    /// own Erlang majors and the newest release as the build patch.
    #[must_use]
    pub fn new(host: HostName, series: HostSeries) -> Self {
        Self {
            host,
            series,
            options: PlanOptions::default(),
        }
    }

    /// Set `--up-to`: the newest release to include, which is the build
    /// patch.
    #[must_use]
    pub fn up_to(mut self, tag: TagName) -> Self {
        self.options.up_to = Some(tag);
        self
    }

    /// Add one `--erlang-major`. Repeatable.
    #[must_use]
    pub fn erlang_major(mut self, major: ErlangMajor) -> Self {
        self.options.erlang_majors.push(major);
        self
    }

    fn push_args(&self, args: &mut Vec<OsString>) {
        args.push(OsString::from("--host"));
        args.push(OsString::from(self.host.to_string()));
        args.push(OsString::from("--series"));
        args.push(OsString::from(self.series.to_string()));
        if let Some(tag) = &self.options.up_to {
            args.push(OsString::from("--up-to"));
            args.push(OsString::from(tag.to_string()));
        }
        for major in &self.options.erlang_majors {
            args.push(OsString::from("--erlang-major"));
            args.push(OsString::from(major.to_string()));
        }
    }
}

/// Where `jigsaw check` reads its plan from.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum CheckTarget {
    /// A plan written by `jigsaw plan --plan-file-path`.
    PlanFile(PathBuf),
    /// A plan drafted from the snapshot store alone.
    Arguments(PlanArguments),
}

/// `jigsaw plan` builder. The host and the series are required.
#[must_use = "a builder has no effect until .run() is called"]
pub struct JigsawPlanBuilder<'a, B: Backend, H: HostState, S: SeriesState> {
    driver: &'a Backhopper<B>,
    host: H,
    series: S,
    options: PlanOptions,
    plan_file_path: Option<PathBuf>,
}

impl<B: Backend, H: HostState, S: SeriesState> fmt::Debug for JigsawPlanBuilder<'_, B, H, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JigsawPlanBuilder")
            .field("options", &self.options)
            .field("plan_file_path", &self.plan_file_path)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, H: HostState, S: SeriesState> JigsawPlanBuilder<'_, B, H, S> {
    /// Set `--up-to`: the newest release to include, which is the build
    /// patch.
    pub fn up_to(mut self, tag: TagName) -> Self {
        self.options.up_to = Some(tag);
        self
    }

    /// Add one `--erlang-major`. Repeatable.
    pub fn erlang_major(mut self, major: ErlangMajor) -> Self {
        self.options.erlang_majors.push(major);
        self
    }

    /// Set `--plan-file-path`: also write the envelope to this file, for
    /// `jigsaw check` and `snapshots generate --from-jigsaw-plan`.
    pub fn plan_file_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.plan_file_path = Some(path.into());
        self
    }
}

impl<'a, B: Backend, S: SeriesState> JigsawPlanBuilder<'a, B, NoHost, S> {
    /// Set `--host`.
    pub fn host(self, host: HostName) -> JigsawPlanBuilder<'a, B, WithHost, S> {
        JigsawPlanBuilder {
            driver: self.driver,
            host: WithHost(host),
            series: self.series,
            options: self.options,
            plan_file_path: self.plan_file_path,
        }
    }
}

impl<'a, B: Backend, H: HostState> JigsawPlanBuilder<'a, B, H, NoSeries> {
    /// Set `--series`.
    pub fn series(self, series: HostSeries) -> JigsawPlanBuilder<'a, B, H, WithSeries> {
        JigsawPlanBuilder {
            driver: self.driver,
            host: self.host,
            series: WithSeries(series),
            options: self.options,
            plan_file_path: self.plan_file_path,
        }
    }
}

impl<B: Backend> JigsawPlanBuilder<'_, B, WithHost, WithSeries> {
    /// The arguments collected so far, for `jigsaw check` to draft the
    /// same plan from the store. The plan file path is not part of them.
    #[must_use]
    pub fn into_arguments(self) -> PlanArguments {
        PlanArguments {
            host: self.host.0,
            series: self.series.0,
            options: self.options,
        }
    }

    /// Dispatch and return the parsed plan. A plan with missing
    /// snapshots exits `3` and is still returned.
    pub fn run(self) -> Result<JigsawPlanPayload, DriverError> {
        self.run_with_diagnostics().map(|(plan, _)| plan)
    }

    /// Dispatch and return the parsed plan plus the diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(JigsawPlanPayload, ExecutedInvocation), DriverError> {
        let driver = self.driver;
        let plan_file_path = self.plan_file_path.clone();
        let mut args = Vec::new();
        self.into_arguments().push_args(&mut args);
        if let Some(path) = plan_file_path {
            args.push(OsString::from("--plan-file-path"));
            args.push(path.into_os_string());
        }
        driver.dispatch_typed::<JigsawPlanPayload>(Verb::JigsawPlan, args, StdinPayload::None)
    }
}

/// `jigsaw check` builder. The plan source and at least one plugin
/// directory are required, and the directories are held by the marker.
#[must_use = "a builder has no effect until .run() is called"]
pub struct JigsawCheckBuilder<'a, B: Backend, T: TargetState, P: PluginsState> {
    driver: &'a Backhopper<B>,
    target: T,
    plugins: P,
}

impl<B: Backend, T: TargetState, P: PluginsState> fmt::Debug for JigsawCheckBuilder<'_, B, T, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JigsawCheckBuilder").finish_non_exhaustive()
    }
}

impl<'a, B: Backend, P: PluginsState> JigsawCheckBuilder<'a, B, NoTarget, P> {
    /// Read the plan from a file written by `jigsaw plan`.
    pub fn plan_file(
        self,
        path: impl Into<PathBuf>,
    ) -> JigsawCheckBuilder<'a, B, WithCheckTarget, P> {
        self.with_target(CheckTarget::PlanFile(path.into()))
    }

    /// Draft the plan from the snapshot store, with the arguments a plan
    /// builder collected.
    pub fn plan_arguments(
        self,
        arguments: PlanArguments,
    ) -> JigsawCheckBuilder<'a, B, WithCheckTarget, P> {
        self.with_target(CheckTarget::Arguments(arguments))
    }

    fn with_target(self, target: CheckTarget) -> JigsawCheckBuilder<'a, B, WithCheckTarget, P> {
        JigsawCheckBuilder {
            driver: self.driver,
            target: WithCheckTarget(target),
            plugins: self.plugins,
        }
    }
}

impl<'a, B: Backend, T: TargetState> JigsawCheckBuilder<'a, B, T, NoPlugins> {
    /// Add the first `--plugin-dir-path`.
    pub fn plugin_dir(self, path: impl Into<PathBuf>) -> JigsawCheckBuilder<'a, B, T, WithPlugins> {
        JigsawCheckBuilder {
            driver: self.driver,
            target: self.target,
            plugins: WithPlugins(NonEmpty::new(path.into(), [])),
        }
    }
}

impl<B: Backend, T: TargetState> JigsawCheckBuilder<'_, B, T, WithPlugins> {
    /// Add one more `--plugin-dir-path`.
    pub fn plugin_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugins.0.push(path.into());
        self
    }
}

impl<B: Backend> JigsawCheckBuilder<'_, B, WithCheckTarget, WithPlugins> {
    /// Dispatch and return the parsed report. A report with findings
    /// exits `3` and is still returned; a plan the store does not stock
    /// exits `65` and is an error.
    pub fn run(self) -> Result<JigsawCheckPayload, DriverError> {
        self.run_with_diagnostics().map(|(report, _)| report)
    }

    /// Dispatch and return the parsed report plus the diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(JigsawCheckPayload, ExecutedInvocation), DriverError> {
        let mut args = Vec::new();
        match &self.target.0 {
            CheckTarget::PlanFile(path) => {
                args.push(OsString::from("--plan-file-path"));
                args.push(path.as_os_str().to_owned());
            }
            CheckTarget::Arguments(arguments) => arguments.push_args(&mut args),
        }
        for dir in &self.plugins.0 {
            args.push(OsString::from("--plugin-dir-path"));
            args.push(dir.as_os_str().to_owned());
        }
        self.driver.dispatch_typed::<JigsawCheckPayload>(
            Verb::JigsawCheck,
            args,
            StdinPayload::None,
        )
    }
}
