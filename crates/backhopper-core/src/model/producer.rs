// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The producer generation behind `self_projects`, `resolver_coverage`,
//! and `fingerprint_version`: the three fields entered the envelope
//! together at schema v12, so a reader has one question, not three.

use std::collections::BTreeSet;

use crate::model::names::ProjectName;
use crate::model::resolver_coverage::ResolverCoverage;

/// The two generations of `check` and `check batch` producer that have
/// ever shipped. A payload with some but not all of the three fields
/// cannot have been written by any released binary; `from_fields` reads
/// it as `BeforeV12`, the reading that withholds most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Producer<'a> {
    BeforeV12,
    Current {
        self_projects: &'a BTreeSet<ProjectName>,
        coverage: &'a ResolverCoverage,
        fingerprint_version: u32,
    },
}

impl<'a> Producer<'a> {
    pub(crate) fn from_fields(
        self_projects: Option<&'a BTreeSet<ProjectName>>,
        coverage: Option<&'a ResolverCoverage>,
        fingerprint_version: Option<u32>,
    ) -> Self {
        match (self_projects, coverage, fingerprint_version) {
            (Some(self_projects), Some(coverage), Some(fingerprint_version)) => Self::Current {
                self_projects,
                coverage,
                fingerprint_version,
            },
            _ => Self::BeforeV12,
        }
    }
}
