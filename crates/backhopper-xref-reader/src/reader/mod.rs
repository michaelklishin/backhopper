// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The `SourceReader` orchestrator.
//!
//! For each `.erl` source it produces a `ModuleData` with exports, imports,
//! behaviour declarations, callback declarations, call sites, and unresolved
//! call sites. Header (`.hrl`) files contribute `-define` macros and
//! `-include`-chain entries via the macro table.
//!
//! Submodules: `scan` (top-level driver and `ModuleBuilder`), `attributes`
//! (`-module`, `-export`, `-define`, `-include`, ...), and `calls`
//! (call-site detection).

use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::path::{Path, PathBuf};
use std::str;

use backhopper_xref_graph::{PathId, PathInterner};

use crate::application::{ApplicationAssignment, ProjectLayout};
use crate::errors::{ReadError, ReadWarning};
use crate::model::{ModuleData, ReadOutput};

mod attributes;
mod calls;
mod scan;

#[derive(Debug, Clone, Default)]
pub struct SourceReader {
    layout: ProjectLayout,
}

impl SourceReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_layout(layout: ProjectLayout) -> Self {
        Self { layout }
    }

    /// Read a single source. Locations are tagged with `PathId::new(0)`;
    /// for multi-file reads use `read_tree`, which threads a real
    /// `PathInterner` so each file gets a distinct id.
    pub fn read_one(
        &self,
        path: &Path,
        source: &str,
    ) -> Result<(Option<ModuleData>, Vec<ReadWarning>), ReadError> {
        self.read_one_with_path_id(PathId::new(0), path, source, &HeaderIndex::empty())
    }

    fn read_one_with_path_id(
        &self,
        path_id: PathId,
        path: &Path,
        source: &str,
        headers: &HeaderIndex,
    ) -> Result<(Option<ModuleData>, Vec<ReadWarning>), ReadError> {
        let mut warnings = Vec::new();

        if source.trim().is_empty() {
            warnings.push(ReadWarning::EmptyFile {
                path: path.to_path_buf(),
            });
            return Ok((None, warnings));
        }

        let app = self.layout.resolve(path);
        if let ApplicationAssignment::Ambiguous { candidates } = &app {
            warnings.push(ReadWarning::AmbiguousApplicationAssignment {
                path: path.to_path_buf(),
                candidates: candidates.iter().map(|a| a.as_str().to_owned()).collect(),
            });
        }

        let mut state = scan::ModuleBuilder::new(path.to_path_buf(), path_id, app);
        scan::scan_attributes(source, &mut state, &mut warnings);
        resolve_includes(&mut state, headers, &mut warnings);
        scan::scan_bodies(source, &mut state);
        Ok((state.into_module(&mut warnings), warnings))
    }

    pub fn read_tree<I>(&self, files: I) -> Result<ReadOutput, ReadError>
    where
        I: IntoIterator<Item = (PathBuf, Vec<u8>)>,
    {
        let collected: Vec<(PathBuf, Vec<u8>)> = files.into_iter().collect();
        let headers = HeaderIndex::build(&collected);
        let mut paths = PathInterner::new();
        let mut modules = Vec::new();
        let mut warnings = Vec::new();
        for (path, bytes) in &collected {
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("erl"))
            {
                continue;
            }
            let text = str::from_utf8(bytes).map_err(|e| ReadError::InvalidUtf8 {
                path: path.clone(),
                offset: e.valid_up_to(),
            })?;
            let path_id = paths.intern(path);
            let (module, ws) = self.read_one_with_path_id(path_id, path, text, &headers)?;
            warnings.extend(ws);
            if let Some(m) = module {
                modules.push(m);
            }
        }
        modules.sort_by(|a, b| a.module.cmp(&b.module));
        Ok(ReadOutput {
            modules,
            warnings,
            paths,
        })
    }
}

/// Pre-built lookup over `.hrl` content keyed by basename and by the
/// `app/include/name.hrl` suffix that `-include_lib` directives use.
struct HeaderIndex {
    by_basename: BTreeMap<String, Vec<HeaderEntry>>,
    by_include_lib: BTreeMap<String, HeaderEntry>,
    by_full_path: BTreeMap<PathBuf, HeaderEntry>,
}

#[derive(Clone)]
struct HeaderEntry {
    path: PathBuf,
    body: String,
}

impl HeaderIndex {
    fn empty() -> Self {
        Self {
            by_basename: BTreeMap::new(),
            by_include_lib: BTreeMap::new(),
            by_full_path: BTreeMap::new(),
        }
    }

    fn build(files: &[(PathBuf, Vec<u8>)]) -> Self {
        let mut out = Self::empty();
        for (path, bytes) in files {
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("hrl"))
            {
                continue;
            }
            let Ok(text) = str::from_utf8(bytes) else {
                continue;
            };
            let entry = HeaderEntry {
                path: path.clone(),
                body: text.to_owned(),
            };
            out.by_full_path.insert(path.clone(), entry.clone());
            if let Some(base) = path.file_name().and_then(|n| n.to_str()) {
                out.by_basename
                    .entry(base.to_owned())
                    .or_default()
                    .push(entry.clone());
            }
            if let Some(key) = include_lib_key(path) {
                out.by_include_lib.insert(key, entry);
            }
        }
        out
    }

    fn find_for_include(&self, source_dir: Option<&Path>, raw: &Path) -> Option<&HeaderEntry> {
        if let Some(dir) = source_dir {
            let candidate = dir.join(raw);
            if let Some(entry) = self.by_full_path.get(&candidate) {
                return Some(entry);
            }
        }
        if let Some(entry) = self.by_full_path.get(raw) {
            return Some(entry);
        }
        let base = raw.file_name().and_then(|n| n.to_str())?;
        self.by_basename.get(base).and_then(|v| v.first())
    }

    fn find_for_include_lib(&self, raw: &Path) -> Option<&HeaderEntry> {
        let key = raw.to_string_lossy();
        if let Some(entry) = self.by_include_lib.get(key.as_ref()) {
            return Some(entry);
        }
        let normalized = key.trim_start_matches("./");
        if let Some(entry) = self.by_include_lib.get(normalized) {
            return Some(entry);
        }
        let base = raw.file_name().and_then(|n| n.to_str())?;
        self.by_basename.get(base).and_then(|v| v.first())
    }
}

fn include_lib_key(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let parts: Vec<&str> = s.split('/').collect();
    let include_idx = parts.iter().position(|p| *p == "include")?;
    if include_idx == 0 {
        return None;
    }
    let app = parts[include_idx - 1];
    let rest = &parts[include_idx..];
    Some(format!("{}/{}", app, rest.join("/")))
}

fn resolve_includes(
    state: &mut scan::ModuleBuilder,
    headers: &HeaderIndex,
    warnings: &mut Vec<ReadWarning>,
) {
    let source_dir = state.path.parent().map(Path::to_path_buf);
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    while !state.include_paths.is_empty() {
        let pending: Vec<PathBuf> = mem::take(&mut state.include_paths);
        for raw in pending {
            let entry = match resolve_one(headers, source_dir.as_deref(), &raw) {
                Some(e) => e,
                None => {
                    warnings.push(ReadWarning::UnresolvedInclude {
                        path: state.path.clone(),
                        target: raw,
                    });
                    continue;
                }
            };
            if !seen.insert(entry.path.clone()) {
                continue;
            }
            scan::fold_header_into(&entry.body, state);
        }
    }
}

fn resolve_one<'a>(
    headers: &'a HeaderIndex,
    source_dir: Option<&Path>,
    raw: &Path,
) -> Option<&'a HeaderEntry> {
    if is_include_lib_shape(raw) {
        if let Some(e) = headers.find_for_include_lib(raw) {
            return Some(e);
        }
    }
    headers.find_for_include(source_dir, raw)
}

fn is_include_lib_shape(raw: &Path) -> bool {
    let s = raw.to_string_lossy();
    s.contains("/include/")
}