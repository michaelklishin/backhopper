// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Verb namespace: a typed enum of every CLI subcommand the driver
//! knows how to invoke, plus a [`VerbId`] that also carries arbitrary
//! verb paths so callers can reach future verbs the driver does not
//! yet wrap.

use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Every verb the driver knows how to invoke.
///
/// `Verb` is the canonical identifier across the driver: builder
/// methods, mock matchers, and tracing fields all key on it instead
/// of free-text command strings. The `cli_path` method returns the
/// argv prefix the CLI expects (`["check", "patch"]` and so on).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Verb {
    /// `backhopper version`.
    Version,
    /// `backhopper doctor`.
    Doctor,
    /// `backhopper config path`.
    ConfigPath,
    /// `backhopper config show`.
    ConfigShow,
    /// `backhopper config validate`.
    ConfigValidate,
    /// `backhopper projects list`.
    ProjectsList,
    /// `backhopper projects show`.
    ProjectsShow,
    /// `backhopper series list`.
    SeriesList,
    /// `backhopper series show`.
    SeriesShow,
    /// `backhopper snapshots list`.
    SnapshotsList,
    /// `backhopper snapshots show`.
    SnapshotsShow,
    /// `backhopper snapshots project_diff`.
    SnapshotsProjectDiff,
    /// `backhopper snapshots series_diff`.
    SnapshotsSeriesDiff,
    /// `backhopper snapshots verify`.
    SnapshotsVerify,
    /// `backhopper snapshots generate`.
    SnapshotsGenerate,
    /// `backhopper snapshots rebuild`.
    SnapshotsRebuild,
    /// `backhopper snapshots migrate`.
    SnapshotsMigrate,
    /// `backhopper snapshots lookup`.
    SnapshotsLookup,
    /// `backhopper check patch`.
    CheckPatch,
    /// `backhopper check commit`.
    CheckCommit,
    /// `backhopper check range`.
    CheckRange,
    /// `backhopper check merge`.
    CheckMerge,
    /// `backhopper check tree`.
    CheckTree,
    /// `backhopper check batch`.
    CheckBatch,
    /// `backhopper siblings doctor`.
    SiblingsDoctor,
    /// `backhopper suites plan`.
    SuitesPlan,
    /// `backhopper cache stats`.
    CacheStats,
    /// `backhopper cache list`.
    CacheList,
    /// `backhopper cache show`.
    CacheShow,
    /// `backhopper cache evict`.
    CacheEvict,
    /// `backhopper cache prune`.
    CachePrune,
    /// `backhopper cache clear`.
    CacheClear,
    /// `backhopper schema show`.
    SchemaShow,
    /// `backhopper schema diff`.
    SchemaDiff,
}

impl Verb {
    /// True for verbs whose source repository must come from the
    /// driver-global `--repo-dir-path`: without it the CLI silently
    /// defaults to the current directory and resolves commits against
    /// the wrong tree.
    #[must_use]
    pub const fn requires_global_repo_dir(self) -> bool {
        matches!(
            self,
            Self::CheckCommit
                | Self::CheckRange
                | Self::CheckMerge
                | Self::CheckBatch
                | Self::SuitesPlan
        )
    }

    /// The argv prefix the CLI expects, e.g. `["check", "patch"]`.
    #[must_use]
    pub const fn cli_path(self) -> &'static [&'static str] {
        match self {
            Self::Version => &["version"],
            Self::Doctor => &["doctor"],
            Self::ConfigPath => &["config", "path"],
            Self::ConfigShow => &["config", "show"],
            Self::ConfigValidate => &["config", "validate"],
            Self::ProjectsList => &["projects", "list"],
            Self::ProjectsShow => &["projects", "show"],
            Self::SeriesList => &["series", "list"],
            Self::SeriesShow => &["series", "show"],
            Self::SnapshotsList => &["snapshots", "list"],
            Self::SnapshotsShow => &["snapshots", "show"],
            Self::SnapshotsProjectDiff => &["snapshots", "project_diff"],
            Self::SnapshotsSeriesDiff => &["snapshots", "series_diff"],
            Self::SnapshotsVerify => &["snapshots", "verify"],
            Self::SnapshotsGenerate => &["snapshots", "generate"],
            Self::SnapshotsRebuild => &["snapshots", "rebuild"],
            Self::SnapshotsMigrate => &["snapshots", "migrate"],
            Self::SnapshotsLookup => &["snapshots", "lookup"],
            Self::CheckPatch => &["check", "patch"],
            Self::CheckCommit => &["check", "commit"],
            Self::CheckRange => &["check", "range"],
            Self::CheckMerge => &["check", "merge"],
            Self::CheckTree => &["check", "tree"],
            Self::CheckBatch => &["check", "batch"],
            Self::SiblingsDoctor => &["siblings", "doctor"],
            Self::SuitesPlan => &["suites", "plan"],
            Self::CacheStats => &["cache", "stats"],
            Self::CacheList => &["cache", "list"],
            Self::CacheShow => &["cache", "show"],
            Self::CacheEvict => &["cache", "evict"],
            Self::CachePrune => &["cache", "prune"],
            Self::CacheClear => &["cache", "clear"],
            Self::SchemaShow => &["schema", "show"],
            Self::SchemaDiff => &["schema", "diff"],
        }
    }

    /// Human-friendly form: `"check patch"`. Stable; included in
    /// error messages, tracing spans, and `Display`.
    #[must_use]
    pub fn display_name(self) -> String {
        self.cli_path().join(" ")
    }

    /// Parse a slash- or space-separated verb path.
    ///
    /// ```
    /// use backhopper_driver::Verb;
    /// assert_eq!(Verb::parse("check patch").unwrap(), Verb::CheckPatch);
    /// assert_eq!(Verb::parse("check/patch").unwrap(), Verb::CheckPatch);
    /// assert_eq!(Verb::parse("version").unwrap(), Verb::Version);
    /// assert!(Verb::parse("nonsense").is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, UnknownVerb> {
        let normalised: Vec<&str> = s
            .split(|c: char| c == '/' || c.is_whitespace())
            .filter(|seg| !seg.is_empty())
            .collect();
        for verb in Self::ALL {
            if verb.cli_path() == normalised.as_slice() {
                return Ok(*verb);
            }
        }
        Err(UnknownVerb(s.to_owned()))
    }

    /// Iterator over every known verb. Useful for capability probes
    /// and exhaustive test enumeration.
    pub fn iter() -> impl Iterator<Item = Verb> {
        Self::ALL.iter().copied()
    }

    const ALL: &'static [Verb] = &[
        Self::Version,
        Self::Doctor,
        Self::ConfigPath,
        Self::ConfigShow,
        Self::ConfigValidate,
        Self::ProjectsList,
        Self::ProjectsShow,
        Self::SeriesList,
        Self::SeriesShow,
        Self::SnapshotsList,
        Self::SnapshotsShow,
        Self::SnapshotsProjectDiff,
        Self::SnapshotsSeriesDiff,
        Self::SnapshotsVerify,
        Self::SnapshotsGenerate,
        Self::SnapshotsRebuild,
        Self::SnapshotsMigrate,
        Self::SnapshotsLookup,
        Self::CheckPatch,
        Self::CheckCommit,
        Self::CheckRange,
        Self::CheckMerge,
        Self::CheckTree,
        Self::CheckBatch,
        Self::SiblingsDoctor,
        Self::SuitesPlan,
        Self::CacheStats,
        Self::CacheList,
        Self::CacheShow,
        Self::CacheEvict,
        Self::CachePrune,
        Self::CacheClear,
        Self::SchemaShow,
        Self::SchemaDiff,
    ];
}

impl Display for Verb {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_name())
    }
}

impl FromStr for Verb {
    type Err = UnknownVerb;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Verb identifier accepted by the escape hatches.
///
/// `VerbId::Known(Verb)` covers every verb the driver wraps;
/// `VerbId::Custom(path)` covers verbs the driver does not yet wrap
/// (or verbs unique to a forked CLI). The custom variant uses a
/// `Cow` so callers can pass either owned or borrowed segments.
#[derive(Debug, Clone)]
pub enum VerbId<'a> {
    /// A verb the driver knows about.
    Known(Verb),
    /// An arbitrary verb path, e.g. `["future", "experiment"]`.
    Custom(Cow<'a, [Cow<'a, str>]>),
}

impl VerbId<'_> {
    /// The argv prefix for this verb.
    #[must_use]
    pub fn cli_path(&self) -> Vec<&str> {
        match self {
            Self::Known(v) => v.cli_path().to_vec(),
            Self::Custom(path) => path.iter().map(AsRef::as_ref).collect(),
        }
    }

    /// Human-friendly form: `"check patch"` or `"future experiment"`.
    #[must_use]
    pub fn display_name(&self) -> String {
        self.cli_path().join(" ")
    }

    /// `Some(verb)` when this id wraps a known verb; `None`
    /// otherwise. Useful for routing the success-shaped exit-code
    /// convention only to known verbs.
    #[must_use]
    pub fn as_known(&self) -> Option<Verb> {
        match self {
            Self::Known(v) => Some(*v),
            Self::Custom(_) => None,
        }
    }

    /// Owned copy detached from any borrowed segments. Used when the
    /// driver needs to carry the id past the originating call frame
    /// (for example, into an [`ExecutedInvocation`]).
    ///
    /// [`ExecutedInvocation`]: crate::envelope::ExecutedInvocation
    #[must_use]
    pub fn into_owned(self) -> VerbId<'static> {
        match self {
            Self::Known(v) => VerbId::Known(v),
            Self::Custom(path) => {
                let owned: Vec<Cow<'static, str>> = path
                    .into_owned()
                    .into_iter()
                    .map(|seg| Cow::Owned(seg.into_owned()))
                    .collect();
                VerbId::Custom(Cow::Owned(owned))
            }
        }
    }
}

impl Display for VerbId<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_name())
    }
}

impl From<Verb> for VerbId<'_> {
    fn from(v: Verb) -> Self {
        Self::Known(v)
    }
}

/// Returned when a verb path cannot be matched to a known [`Verb`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown verb path: {0:?}")]
pub struct UnknownVerb(pub String);
