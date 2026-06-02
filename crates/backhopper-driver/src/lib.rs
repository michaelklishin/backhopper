// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

use std::fmt;

pub mod backend;
pub mod builder;
pub mod cancellation;
pub mod driver;
pub mod envelope;
pub mod error;
pub mod exit;
pub mod options;
pub mod selector;
pub mod stdin;
pub mod subprocess;
pub mod types;
pub mod verb;

#[cfg(feature = "mock")]
#[cfg_attr(docsrs, doc(cfg(feature = "mock")))]
pub mod mock;

pub use backend::{Backend, Invocation, OutputChannel, OutputPolicy, RawOutcome};
pub use builder::check::{
    AggregateVerdict, Check, CheckCommitBuilder, CheckMergeBuilder, CheckOptions,
    CheckPatchBuilder, CheckRangeBuilder, ExplainFormat, PatchSource, PinDescriptor,
    QueriedAgainst, SeriesEvaluation, VerdictKind,
};
pub use builder::state::{InputState, NoInput, NoTarget, TargetState, WithInput, WithTarget};
pub use cancellation::{Aborter, CancellationToken};
pub use driver::{Backhopper, VersionInfo};
pub use envelope::{Envelope, EnvelopeWarning, ExecutedInvocation, SchemaVersion};
pub use error::{DriverError, ErrorKind, Result};
pub use exit::ExitClass;
pub use options::{EnvInheritance, EnvOverlay, GlobalOptions, GlobalOptionsBuilder};
pub use selector::PinSelector;
pub use stdin::StdinPayload;
pub use subprocess::SubprocessBackend;
pub use verb::{UnknownVerb, Verb, VerbId};

/// Range of envelope schema versions this driver release understands.
pub const SUPPORTED_SCHEMA: SchemaVersionRange = SchemaVersionRange {
    min: SchemaVersion::new(1),
    max: SchemaVersion::new(1),
};

/// Inclusive range of envelope schema versions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SchemaVersionRange {
    /// Lowest supported.
    pub min: SchemaVersion,
    /// Highest supported.
    pub max: SchemaVersion,
}

impl SchemaVersionRange {
    /// Build a single-version range.
    #[must_use]
    pub const fn singleton(v: SchemaVersion) -> Self {
        Self { min: v, max: v }
    }

    /// True when `v` is inside the range.
    #[must_use]
    pub const fn contains(self, v: SchemaVersion) -> bool {
        v.get() >= self.min.get() && v.get() <= self.max.get()
    }
}

impl fmt::Display for SchemaVersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..={}", self.min, self.max)
    }
}

impl fmt::Debug for SchemaVersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
