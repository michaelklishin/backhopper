// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Process-wide options applied to every invocation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use crate::cancellation::CancellationToken;

/// Options shared across every invocation a [`Backhopper`] makes.
///
/// `GlobalOptions` is `#[non_exhaustive]`: adding fields is
/// non-breaking. `Default::default()` is the "do not override anything"
/// shape; `builder()` is the idiomatic chained constructor.
///
/// [`Backhopper`]: crate::Backhopper
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct GlobalOptions {
    /// `--config-file-path` path to `backhopper.toml`.
    pub config_path: Option<PathBuf>,
    /// `--snapshot-dir-path` override.
    pub snapshot_dir: Option<PathBuf>,
    /// `--repo-dir-path` for self-pin resolution.
    pub repo_dir_path: Option<PathBuf>,
    /// Working directory the child runs in. `None` inherits the
    /// caller's cwd.
    pub working_directory: Option<PathBuf>,
    /// Pass `--non-interactive`. Default `true` because the driver
    /// is by definition non-interactive; set `false` to allow
    /// interactive prompts from a wrapping shell.
    pub non_interactive: bool,
    /// Pass `--quiet`.
    pub quiet: bool,
    /// Pass `--verbose`.
    pub verbose: bool,
    /// Pass `--dry-run`.
    pub dry_run: bool,
    /// Per-invocation timeout. `None` means no driver-imposed
    /// timeout (the OS still terminates via signals).
    pub timeout: Option<Duration>,
    /// Environment variables passed to the child.
    pub env: EnvOverlay,
    /// Cooperative cancellation token. When set and tripped, the
    /// backend signals the child.
    pub cancellation: Option<CancellationToken>,
}

impl GlobalOptions {
    /// Idiomatic chained constructor.
    ///
    /// ```
    /// use backhopper_driver::GlobalOptions;
    /// use std::time::Duration;
    ///
    /// let opts = GlobalOptions::builder()
    ///     .quiet(true)
    ///     .timeout(Duration::from_secs(30))
    ///     .build();
    /// assert!(opts.quiet);
    /// assert_eq!(opts.timeout, Some(Duration::from_secs(30)));
    /// ```
    pub fn builder() -> GlobalOptionsBuilder {
        GlobalOptionsBuilder::default()
    }

    pub(crate) fn non_interactive_default() -> Self {
        Self {
            non_interactive: true,
            ..Self::default()
        }
    }
}

/// Chained constructor for [`GlobalOptions`].
#[derive(Debug, Default)]
#[must_use = "build() consumes the builder; assign the result"]
pub struct GlobalOptionsBuilder {
    inner: GlobalOptions,
}

impl GlobalOptionsBuilder {
    /// Override `-c`.
    pub fn config_path(mut self, p: impl Into<PathBuf>) -> Self {
        self.inner.config_path = Some(p.into());
        self
    }

    /// Override `--snapshot-dir`.
    pub fn snapshot_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.inner.snapshot_dir = Some(p.into());
        self
    }

    /// Set `--repo-dir-path`.
    pub fn repo_dir_path(mut self, p: impl Into<PathBuf>) -> Self {
        self.inner.repo_dir_path = Some(p.into());
        self
    }

    /// Run the child with the named working directory.
    pub fn working_directory(mut self, p: impl Into<PathBuf>) -> Self {
        self.inner.working_directory = Some(p.into());
        self
    }

    /// Toggle `--non-interactive`.
    pub fn non_interactive(mut self, on: bool) -> Self {
        self.inner.non_interactive = on;
        self
    }

    /// Toggle `--quiet`.
    pub fn quiet(mut self, on: bool) -> Self {
        self.inner.quiet = on;
        self
    }

    /// Toggle `--verbose`.
    pub fn verbose(mut self, on: bool) -> Self {
        self.inner.verbose = on;
        self
    }

    /// Toggle `--dry-run`.
    pub fn dry_run(mut self, on: bool) -> Self {
        self.inner.dry_run = on;
        self
    }

    /// Set the per-invocation timeout.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.inner.timeout = Some(d);
        self
    }

    /// Replace the environment overlay.
    pub fn env(mut self, env: EnvOverlay) -> Self {
        self.inner.env = env;
        self
    }

    /// Attach a cooperative cancellation token.
    pub fn cancellation(mut self, token: CancellationToken) -> Self {
        self.inner.cancellation = Some(token);
        self
    }

    /// Finalise the builder.
    #[must_use]
    pub fn build(self) -> GlobalOptions {
        self.inner
    }
}

/// Environment inheritance policy plus per-key overrides.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct EnvOverlay {
    /// What the child sees in addition to `overrides`.
    pub inheritance: EnvInheritance,
    /// Variables that take precedence over `inheritance`.
    pub overrides: BTreeMap<OsString, OsString>,
}

impl EnvOverlay {
    /// Inherit everything (the default).
    #[must_use]
    pub fn inherit() -> Self {
        Self::default()
    }

    /// Inherit nothing; `overrides` is the entire child environment.
    #[must_use]
    pub fn clear() -> Self {
        Self {
            inheritance: EnvInheritance::Clear,
            overrides: BTreeMap::new(),
        }
    }

    /// Inherit only the named variables.
    #[must_use]
    pub fn allow_list(names: &'static [&'static str]) -> Self {
        Self {
            inheritance: EnvInheritance::AllowList(names),
            overrides: BTreeMap::new(),
        }
    }

    /// Add or replace a single override.
    #[must_use]
    pub fn with(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.overrides.insert(key.into(), value.into());
        self
    }
}

/// What the child inherits from the calling process's environment.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvInheritance {
    /// Inherit the caller's full environment. The default.
    #[default]
    Inherit,
    /// Clear the environment; only `overrides` is visible.
    Clear,
    /// Inherit only the named variables; ignore the rest.
    AllowList(&'static [&'static str]),
}
