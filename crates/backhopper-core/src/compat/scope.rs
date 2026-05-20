// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-pin and series-wide tracked-scope filters.
//!
//! A call site `mod:fun(...)` only deserves a verdict when `mod` belongs
//! to a project the operator is actually tracking. Otherwise its
//! "absence" from the pin's snapshot is meaningless.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use crate::model::names::{ModuleName, ProjectName, RecordName};
use crate::model::snapshot::{Snapshot, state};

/// Decides whether a call site or a touched path is tracked for one pin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinScope {
    modules: BTreeSet<ModuleName>,
    records: BTreeSet<RecordName>,
    path_prefixes: Vec<String>,
}

impl PinScope {
    pub fn from_snapshot(
        project: ProjectName,
        snapshot: &Snapshot<state::Canonical>,
        extra_modules: impl IntoIterator<Item = ModuleName>,
    ) -> Self {
        let mut modules: BTreeSet<ModuleName> =
            snapshot.modules().iter().map(|m| m.name.clone()).collect();
        modules.extend(extra_modules);
        let records: BTreeSet<RecordName> = snapshot
            .headers()
            .iter()
            .flat_map(|h| h.records.iter().map(|r| r.name.clone()))
            .collect();
        Self {
            modules,
            records,
            path_prefixes: vec![format!("deps/{project}/")],
        }
    }

    pub fn contains_module(&self, module: &ModuleName) -> bool {
        self.modules.contains(module)
    }

    pub fn contains_record(&self, record: &RecordName) -> bool {
        self.records.contains(record)
    }

    pub fn modules(&self) -> &BTreeSet<ModuleName> {
        &self.modules
    }

    pub fn records(&self) -> &BTreeSet<RecordName> {
        &self.records
    }

    /// Rewrites a patch path into a project-local path, or returns
    /// `None` when the path belongs to a different project.
    pub fn rewrite_path<'p>(&self, path: &'p Path) -> Option<&'p Path> {
        let s = path.to_str()?;
        for prefix in &self.path_prefixes {
            if let Some(rest) = s.strip_prefix(prefix.as_str()) {
                return Some(Path::new(rest));
            }
        }
        if self.path_prefixes.is_empty() || !s.starts_with("deps/") {
            return Some(path);
        }
        None
    }
}

/// Parses `extra_modules` from raw config strings, silently dropping any
/// entry that is not a valid module name.
pub fn parse_module_names<'a>(raw: impl IntoIterator<Item = &'a String>) -> Vec<ModuleName> {
    raw.into_iter()
        .filter_map(|s| ModuleName::from_str(s).ok())
        .collect()
}

/// Aggregates untracked module calls observed across a series.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UntrackedTally {
    counts: BTreeMap<ModuleName, usize>,
}

impl UntrackedTally {
    pub fn record(&mut self, module: ModuleName) {
        *self.counts.entry(module).or_insert(0) += 1;
    }

    pub fn into_map(self) -> BTreeMap<ModuleName, usize> {
        self.counts
    }
}