// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `suites`-family verb builders.
//!
//! `suites plan` takes the modified-path list as its one required
//! input, threaded through a type-state marker; the repository comes
//! from the driver-global `--repo-dir-path`.

use std::ffi::OsString;
use std::fmt;
use std::marker::PhantomData;
use std::path::PathBuf;

use backhopper_core::model::names::ApplicationName;
use backhopper_core::suites::SuitePlan;

use crate::backend::Backend;
use crate::builder::state::{InputState, NoInput, WithInput};
use crate::driver::Backhopper;
use crate::envelope::ExecutedInvocation;
use crate::error::DriverError;
use crate::stdin::StdinPayload;
use crate::verb::Verb;

/// Top-level handle returned by [`Backhopper::suites`].
#[derive(Debug)]
pub struct Suites<'a, B: Backend> {
    pub(crate) driver: &'a Backhopper<B>,
}

impl<'a, B: Backend> Suites<'a, B> {
    /// Build a `suites plan` invocation. The repository comes from the
    /// driver-global `--repo-dir-path`.
    pub fn plan(&'a self) -> SuitesPlanBuilder<'a, B, NoInput> {
        SuitesPlanBuilder {
            driver: self.driver,
            modified_paths: None,
            auto_library_apps: false,
            library_apps: Vec::new(),
            extra_rules_file_path: None,
            _state: PhantomData,
        }
    }
}

/// Where the modified-path list comes from.
#[derive(Debug, Clone)]
enum ModifiedPaths {
    // Newline-framed paths sent on stdin; the CLI reads `-`.
    Bytes(Vec<u8>),
    // A file of one path per line, passed as the flag value.
    File(PathBuf),
}

/// `suites plan` builder. The modified-path list is the one required
/// input; everything else is optional.
#[must_use = "a builder has no effect until .run() is called"]
pub struct SuitesPlanBuilder<'a, B: Backend, I: InputState> {
    driver: &'a Backhopper<B>,
    modified_paths: Option<ModifiedPaths>,
    auto_library_apps: bool,
    library_apps: Vec<ApplicationName>,
    extra_rules_file_path: Option<PathBuf>,
    _state: PhantomData<I>,
}

impl<B: Backend, I: InputState> fmt::Debug for SuitesPlanBuilder<'_, B, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SuitesPlanBuilder")
            .field("auto_library_apps", &self.auto_library_apps)
            .field("library_apps", &self.library_apps)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, I: InputState> SuitesPlanBuilder<'_, B, I> {
    /// Set `--auto-library-apps`.
    pub fn auto_library_apps(mut self, on: bool) -> Self {
        self.auto_library_apps = on;
        self
    }

    /// Add one `--library-app`. Repeatable.
    pub fn library_app(mut self, app: impl Into<ApplicationName>) -> Self {
        self.library_apps.push(app.into());
        self
    }

    /// Set `--extra-rules-file-path`: a TOML file of extra suite rules
    /// unioned with the loaded config. Needs a binary that ships the
    /// flag (a `0.16.0` floor).
    pub fn extra_rules_file_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.extra_rules_file_path = Some(path.into());
        self
    }
}

impl<'a, B: Backend> SuitesPlanBuilder<'a, B, NoInput> {
    /// Frame an iterator of modified paths onto stdin.
    pub fn modified_paths(
        self,
        paths: impl IntoIterator<Item = impl Into<PathBuf>>,
    ) -> SuitesPlanBuilder<'a, B, WithInput> {
        let mut buf = Vec::new();
        for p in paths {
            buf.extend_from_slice(p.into().to_string_lossy().as_bytes());
            buf.push(b'\n');
        }
        self.with_input(ModifiedPaths::Bytes(buf))
    }

    /// Pass a file of one modified path per line as
    /// `--modified-paths-file-path`.
    pub fn modified_paths_file_path(
        self,
        path: impl Into<PathBuf>,
    ) -> SuitesPlanBuilder<'a, B, WithInput> {
        self.with_input(ModifiedPaths::File(path.into()))
    }

    fn with_input(self, modified_paths: ModifiedPaths) -> SuitesPlanBuilder<'a, B, WithInput> {
        SuitesPlanBuilder {
            driver: self.driver,
            modified_paths: Some(modified_paths),
            auto_library_apps: self.auto_library_apps,
            library_apps: self.library_apps,
            extra_rules_file_path: self.extra_rules_file_path,
            _state: PhantomData,
        }
    }
}

impl<B: Backend> SuitesPlanBuilder<'_, B, WithInput> {
    /// Dispatch and return the parsed plan.
    pub fn run(self) -> Result<SuitePlan, DriverError> {
        self.run_with_diagnostics().map(|(plan, _)| plan)
    }

    /// Dispatch and return the parsed plan plus the diagnostic snapshot.
    pub fn run_with_diagnostics(self) -> Result<(SuitePlan, ExecutedInvocation), DriverError> {
        let modified_paths = self.modified_paths.as_ref().expect("input set");
        let mut args = Vec::new();
        if self.auto_library_apps {
            args.push(OsString::from("--auto-library-apps"));
        }
        for app in &self.library_apps {
            args.push(OsString::from("--library-app"));
            args.push(OsString::from(app.to_string()));
        }
        if let Some(path) = &self.extra_rules_file_path {
            args.push(OsString::from("--extra-rules-file-path"));
            args.push(path.as_os_str().to_owned());
        }
        args.push(OsString::from("--modified-paths-file-path"));
        let stdin = match modified_paths {
            ModifiedPaths::Bytes(b) => {
                args.push(OsString::from("-"));
                StdinPayload::Bytes(b.as_slice())
            }
            ModifiedPaths::File(p) => {
                args.push(p.as_os_str().to_owned());
                StdinPayload::None
            }
        };
        self.driver
            .dispatch_typed::<SuitePlan>(Verb::SuitesPlan, args, stdin)
    }
}
