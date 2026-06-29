// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The macro-environment hash behind the verdict cache's content key.
//!
//! Identical patch bytes do not imply identical verdicts: the
//! analyzer expands `?MACRO` references in the patch's hunk lines
//! against per-file macro tables built from the parent tree. This
//! module rebuilds exactly those tables — through the same
//! `build_macro_table` fold core uses, against a resolver that
//! mirrors `FileMap`'s lookup order over the parent tree — then
//! restricts each table to the closure of macro names the patch can
//! actually reach (the names in its hunk lines, plus names reachable
//! through their expansions) and hashes the restriction. A define the
//! patch never references cannot influence the analysis, so
//! unrelated header drift between branches does not bust the cascade
//! reuse the content level exists for.
//!
//! Soundness rules: the name seed is a superset of what the analyzer
//! looks up (every `?name` token on every hunk line, any case, plus
//! all arities per name); and whenever the mirror cannot be exact (a
//! candidate blob that is not UTF-8, an unreadable object, a
//! quoted-atom macro reference) the environment is reported
//! unresolvable and the caller degrades to a per-commit content key —
//! a guaranteed miss, never a false hit.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use backhopper_cache::content_hash;
use backhopper_core::compat::patch::{HunkLine, PatchedFile};
use backhopper_core::compat::source_macros::{HeaderBody, HeaderResolver, build_macro_table};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::model::names::CommitSha;
use backhopper_git::GitRepo;

static MACRO_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\?([A-Za-z_@][A-Za-z0-9_@]*)").expect("macro name regex must compile")
});

/// Rebuild the patch-reachable slice of the touched files' macro
/// tables at `diff_base` and hash it. `None` when the mirror cannot
/// be exact.
pub fn macro_environment_hash(
    repo: &GitRepo,
    diff_base: &CommitSha,
    files: &[PatchedFile],
) -> Option<String> {
    let mut seeded: Vec<(&Path, BTreeSet<String>)> = Vec::new();
    for file in files {
        // the same path preference the analyzer uses for its lookup
        let Some(path) = file.primary_path() else {
            continue;
        };
        // None marks a quoted-atom macro reference the seed cannot model
        let seed = seed_names(file)?;
        if !seed.is_empty() {
            seeded.push((path, seed));
        }
    }
    // restricted tables keyed by path string; BTreeMap keeps the
    // hash input ordered
    let mut tables: BTreeMap<String, Vec<(String, Option<u8>, String)>> = BTreeMap::new();
    if !seeded.is_empty() {
        // macro-free patches never pay for the tree listing
        let listing = repo.list_paths_at_commit(diff_base).ok()?;
        let resolver = MirrorResolver::new(repo, diff_base, &listing);
        for (path, seed) in seeded {
            // a path absent from the parent tree (added by the patch)
            // has no macro table, same as FileMap.get returning None
            if !resolver.exact.contains(path) {
                continue;
            }
            let Some(body) = resolver.read_utf8(path) else {
                // FileMap would have skipped a non-UTF-8 file at
                // insert time; mirroring that exactly is not worth
                // the risk
                if resolver.poisoned() {
                    return None;
                }
                continue;
            };
            let table = build_macro_table(&body, path, &resolver);
            if resolver.poisoned() {
                return None;
            }
            let rows = restrict_to_closure(&table, seed);
            if !rows.is_empty() {
                tables.insert(path.to_string_lossy().into_owned(), rows);
            }
        }
    }
    content_hash(&tables).ok()
}

/// Every `?name` token on the file's hunk lines: a superset of what
/// the analyzer can look up. `None` when a quoted-atom macro
/// (`?'...'`) appears, which the token model cannot represent.
fn seed_names(file: &PatchedFile) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for hunk in &file.hunks {
        for line in &hunk.lines {
            let text = match line {
                HunkLine::Added(t) | HunkLine::Removed(t) | HunkLine::Context(t) => t,
            };
            if text.contains("?'") {
                return None;
            }
            for caps in MACRO_NAME.captures_iter(text) {
                names.insert(caps[1].to_owned());
            }
        }
    }
    Some(names)
}

/// Project the table onto the closure of `seed` under expansion: a
/// macro body can reference further macros, which the analyzer
/// re-scans, so those join the slice too. All arities of a name are
/// kept (the analyzer's lookup-by-shape is a subset of that).
fn restrict_to_closure(
    table: &MacroTable,
    seed: BTreeSet<String>,
) -> Vec<(String, Option<u8>, String)> {
    let mut closure = seed;
    let mut worklist: Vec<String> = closure.iter().cloned().collect();
    let mut rows: Vec<(String, Option<u8>, String)> = Vec::new();
    while let Some(name) = worklist.pop() {
        for (key, value) in table.iter().filter(|(k, _)| k.name == name) {
            rows.push((key.name.clone(), key.arity, value.clone()));
            for caps in MACRO_NAME.captures_iter(value) {
                if closure.insert(caps[1].to_owned()) {
                    worklist.push(caps[1].to_owned());
                }
            }
        }
    }
    rows.sort();
    rows
}

/// Mirrors `FileMap`'s four-step include resolution over a parent
/// tree, reading blobs on demand: exact path relative to the source
/// file, exact path as given, the `<app>/include/...` `include_lib`
/// suffix, then first-by-sorted-path basename match.
struct MirrorResolver<'a> {
    repo: &'a GitRepo,
    diff_base: &'a CommitSha,
    exact: BTreeSet<&'a Path>,
    by_include_lib: BTreeMap<String, &'a Path>,
    by_basename: BTreeMap<String, &'a Path>,
    bodies: RefCell<BTreeMap<PathBuf, Option<String>>>,
    poisoned: RefCell<bool>,
}

impl<'a> MirrorResolver<'a> {
    fn new(repo: &'a GitRepo, diff_base: &'a CommitSha, listing: &'a [PathBuf]) -> Self {
        let mut exact: BTreeSet<&Path> = BTreeSet::new();
        let mut by_include_lib: BTreeMap<String, &Path> = BTreeMap::new();
        let mut by_basename: BTreeMap<String, &Path> = BTreeMap::new();
        // the listing is sorted; FileMap keeps the first insert per
        // basename and the last per include_lib key
        for path in listing {
            let s = path.to_string_lossy();
            if !(s.ends_with(".erl") || s.ends_with(".hrl")) {
                continue;
            }
            exact.insert(path.as_path());
            if let Some(key) = include_lib_key(&s) {
                by_include_lib.insert(key, path.as_path());
            }
            if let Some(base) = path.file_name().and_then(|n| n.to_str()) {
                by_basename.entry(base.to_owned()).or_insert(path.as_path());
            }
        }
        Self {
            repo,
            diff_base,
            exact,
            by_include_lib,
            by_basename,
            bodies: RefCell::new(BTreeMap::new()),
            poisoned: RefCell::new(false),
        }
    }

    fn poisoned(&self) -> bool {
        *self.poisoned.borrow()
    }

    /// Read one blob as UTF-8, memoized. A read failure or non-UTF-8
    /// content poisons the mirror: `FileMap` would have resolved past
    /// such a file, and matching that exactly cannot be guaranteed.
    fn read_utf8(&self, path: &Path) -> Option<String> {
        if let Some(cached) = self.bodies.borrow().get(path) {
            return cached.clone();
        }
        let body = match self.repo.read_blob_at(self.diff_base, path) {
            Ok(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(text) => Some(text),
                Err(_) => {
                    *self.poisoned.borrow_mut() = true;
                    None
                }
            },
            Ok(None) | Err(_) => {
                *self.poisoned.borrow_mut() = true;
                None
            }
        };
        self.bodies
            .borrow_mut()
            .insert(path.to_path_buf(), body.clone());
        body
    }

    fn lookup_path(&self, source_dir: Option<&Path>, raw: &Path) -> Option<PathBuf> {
        if let Some(dir) = source_dir {
            let candidate = dir.join(raw);
            if self.exact.contains(candidate.as_path()) {
                return Some(candidate);
            }
        }
        if self.exact.contains(raw) {
            return Some(raw.to_path_buf());
        }
        let key = raw.to_string_lossy();
        if let Some(path) = self.by_include_lib.get(key.as_ref()) {
            return Some(path.to_path_buf());
        }
        let base = raw.file_name().and_then(|n| n.to_str())?;
        self.by_basename.get(base).map(|p| p.to_path_buf())
    }
}

impl HeaderResolver for MirrorResolver<'_> {
    fn resolve(&self, source_dir: Option<&Path>, raw: &Path) -> Option<HeaderBody> {
        let path = self.lookup_path(source_dir, raw)?;
        let body = self.read_utf8(&path)?;
        Some(HeaderBody { path, body })
    }
}

/// Mirror of `FileMap`'s `include_lib_key`: the `<app>/include/...`
/// suffix an `-include_lib` argument names.
fn include_lib_key(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    let include_idx = parts.iter().position(|p| *p == "include")?;
    if include_idx == 0 {
        return None;
    }
    let app = parts[include_idx - 1];
    let tail = parts[include_idx..].join("/");
    Some(format!("{app}/{tail}"))
}
