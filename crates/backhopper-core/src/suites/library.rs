// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `library_apps = "auto"` derivation.
//!
//! A "library" application is one that other applications in the
//! same workspace depend on. We derive the set from the union of
//! every app's `applications` field: an app `L` is library-like when
//! at least `MIN_DEPENDENTS` other in-tree apps list it as a dep.
//!
//! Pure leaves (depended on by nothing) and 1-of-1 dependencies are
//! excluded: their changes ripple to at most one consumer, and the
//! same-app rule already catches that case.

use std::collections::{BTreeMap, BTreeSet};

use crate::app_src::AppSrcSpec;
use crate::model::names::ApplicationName;

const MIN_DEPENDENTS: usize = 2;

/// Returns the apps that look like libraries per the heuristic above.
/// Order is alphabetical by name.
pub fn derive_library_apps(apps: &[AppSrcSpec]) -> Vec<ApplicationName> {
    let in_tree: BTreeSet<&ApplicationName> = apps.iter().map(|a| &a.name).collect();
    let mut dependents: BTreeMap<ApplicationName, usize> = BTreeMap::new();
    for app in apps {
        for dep in &app.applications {
            // OTP standard libs aren't in-tree, so they can't pull cross-app suites in.
            if !in_tree.contains(dep) {
                continue;
            }
            if &app.name == dep {
                continue;
            }
            *dependents.entry(dep.clone()).or_insert(0) += 1;
        }
    }
    dependents
        .into_iter()
        .filter(|(_, n)| *n >= MIN_DEPENDENTS)
        .map(|(name, _)| name)
        .collect()
}
