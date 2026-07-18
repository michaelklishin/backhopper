// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! File-system scanning helpers for suite selection.

use std::fs;
use std::path::Path;

use crate::app_src::{AppSrcSpec, SKIP_DIRS};
use crate::model::names::{ApplicationName, ModuleName};
use crate::suites::model::SuiteRef;

pub(crate) use crate::app_src::{app_root_dir, application_of_path};

/// Enumerates `_SUITE.erl` files under each application's `test/`
/// subdirectory.
pub(crate) fn enumerate_suites(repo_root: &Path, apps: &[AppSrcSpec]) -> Vec<SuiteRef> {
    let mut out: Vec<SuiteRef> = Vec::new();
    for app in apps {
        let test_dir = repo_root.join(app_root_dir(&app.path)).join("test");
        if !test_dir.is_dir() {
            continue;
        }
        collect_suites_in(&app.name, &test_dir, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn collect_suites_in(app: &ApplicationName, dir: &Path, out: &mut Vec<SuiteRef>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && SKIP_DIRS.contains(&name)
            {
                continue;
            }
            collect_suites_in(app, &p, out);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(base) = name.strip_suffix(".erl")
            && base.ends_with("_SUITE")
            && let Ok(module) = ModuleName::new(base)
        {
            out.push(SuiteRef {
                application: app.clone(),
                module,
                path: p,
            });
        }
    }
}
