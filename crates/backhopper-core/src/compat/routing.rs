// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-pin path routing: which configured project owns each touched
//! path, so a pin's verdict can promote to `OutOfScopeFor` or
//! `Untracked` instead of firing a misleading `FileAbsent` row.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config::Project;
use crate::model::names::ProjectName;

/// Path-routing summary for one pin's evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathRouting {
    pub any_in_scope: bool,
    pub out_of_scope_owners: BTreeSet<ProjectName>,
    pub untracked_count: usize,
}

impl PathRouting {
    pub fn first_out_of_scope_owner(&self) -> Option<&ProjectName> {
        self.out_of_scope_owners.first()
    }

    pub fn has_any_attribution(&self) -> bool {
        !self.out_of_scope_owners.is_empty() || self.untracked_count > 0
    }
}

// True when path lives under scan_paths, app_roots, or `deps/{name}/`;
// projects with no scope configured fall back to "owns everything"
pub fn project_owns_path(project: &Project, path: &Path) -> bool {
    project_owns_candidate(project, &path.to_string_lossy())
}

fn project_owns_candidate(project: &Project, candidate: &str) -> bool {
    if project.owns_path(Path::new(candidate)) {
        return true;
    }
    if !project.is_self() {
        let prefix_start = "deps/";
        let name = project.name.as_str();
        if candidate.len() > prefix_start.len() + name.len()
            && candidate.starts_with(prefix_start)
            && candidate[prefix_start.len()..].starts_with(name)
            && candidate.as_bytes()[prefix_start.len() + name.len()] == b'/'
        {
            return true;
        }
    }
    project.scan_paths.is_empty() && project.app_roots.is_empty()
}

pub fn classify_paths_for_pin(
    touched: &[PathBuf],
    pin_project: &Project,
    siblings: &[&Project],
) -> PathRouting {
    let mut routing = PathRouting::default();
    for p in touched {
        let candidate = p.to_string_lossy();
        if project_owns_candidate(pin_project, &candidate) {
            routing.any_in_scope = true;
            continue;
        }
        let owner = siblings
            .iter()
            .find(|s| s.name != pin_project.name && project_owns_candidate(s, &candidate))
            .map(|s| s.name.clone());
        match owner {
            Some(name) => {
                routing.out_of_scope_owners.insert(name);
            }
            None => routing.untracked_count += 1,
        }
    }
    routing
}
