// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! File-system scanning helpers for suite selection.

use std::fs;
use std::path::{Path, PathBuf};

use crate::app_src::AppSrcSpec;
use crate::model::names::{ApplicationName, ModuleName};
use crate::suites::model::SuiteRef;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "_build",
    "_rel",
    "logs",
    "node_modules",
    ".direnv",
    "target",
];

/// Resolves a file path under `repo_root` to the application it
/// belongs to. Strategy: find the app whose `.app.src` file lives
/// in the deepest shared prefix of `path`.
pub(crate) fn application_of_path<'a>(
    apps: &'a [AppSrcSpec],
    repo_root: &Path,
    path: &Path,
) -> Option<&'a ApplicationName> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    let mut best: Option<(&'a ApplicationName, usize)> = None;
    for app in apps {
        let app_dir = app_root_dir(&app.path);
        if abs.starts_with(&app_dir) {
            let depth = app_dir.components().count();
            if best.map(|(_, d)| depth > d).unwrap_or(true) {
                best = Some((&app.name, depth));
            }
        }
    }
    best.map(|(name, _)| name)
}

/// Returns the directory containing the `*.app.src` file's enclosing
/// application: one level up from `src/` if the `.app.src` lives in
/// `src/`, otherwise the file's parent.
fn app_root_dir(app_src_path: &Path) -> PathBuf {
    let parent = app_src_path.parent().unwrap_or(Path::new(""));
    if parent
        .file_name()
        .is_some_and(|n| n == "src" || n == "ebin")
    {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

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
