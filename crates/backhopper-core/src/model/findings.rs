// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Row-level symbol-axis findings resolved against the target tree.
//! The same reasons also merge into applicable pin verdicts; this
//! record is the copy that survives inapplicable ones, so a suite-only
//! pick cannot silently drop what the target checks found.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::resolver_coverage::ResolverClass;
use crate::model::verdict::Reason;

/// Symbol-axis findings for one row, resolved against the target tree.
/// Present exactly when a target context was supplied; independent of
/// every pin verdict, so an inapplicable row still carries its
/// findings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TargetFindings {
    /// Every reason the live-axis target checks produced for this row,
    /// in production order.
    pub reasons: Vec<Reason>,
}

impl TargetFindings {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reasons.is_empty()
    }

    /// Finding counts by resolver class, for ranking.
    #[must_use]
    pub fn by_class(&self) -> Vec<(ResolverClass, usize)> {
        let mut counts: BTreeMap<ResolverClass, usize> = BTreeMap::new();
        for reason in &self.reasons {
            if let Some(class) = reason.resolver_class() {
                *counts.entry(class).or_insert(0) += 1;
            }
        }
        counts.into_iter().collect()
    }

    /// Findings with no resolver class, so the class view never hides
    /// a reason.
    #[must_use]
    pub fn unclassified(&self) -> usize {
        self.reasons
            .iter()
            .filter(|r| r.resolver_class().is_none())
            .count()
    }

    #[must_use]
    pub fn blocking(&self) -> bool {
        self.reasons.iter().any(Reason::is_blocking)
    }
}
