// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Build a per-file `MacroTable` from raw `.erl` and `.hrl` source.
//!
//! Walks the file for `-define`, `-include`, and `-include_lib`
//! attributes and folds resolved header content back in until the
//! include chain stabilises. Cycles are broken by a `seen` set.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::erlang_macros::{MacroTable, parse_define};

/// Look up the body of a header by its raw `-include(...)` or
/// `-include_lib(...)` argument, given the directory of the source
/// file that named it.
pub trait HeaderResolver {
    fn resolve(&self, source_dir: Option<&Path>, raw: &Path) -> Option<HeaderBody>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderBody {
    pub path: PathBuf,
    pub body: String,
}

/// Builds the macro table for `source` (presumed `.erl`), resolving
/// included headers through `resolver`. `source_path` is the path of
/// the file being analyzed: it's used as the include-search anchor.
pub fn build_macro_table(
    source: &str,
    source_path: &Path,
    resolver: &dyn HeaderResolver,
) -> MacroTable {
    let mut macros = MacroTable::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut pending: Vec<PathBuf> = Vec::new();
    fold_attributes(source, &mut macros, &mut pending);
    let source_dir = source_path.parent().map(Path::to_path_buf);
    while let Some(raw) = pending.pop() {
        let Some(entry) = resolver.resolve(source_dir.as_deref(), &raw) else {
            continue;
        };
        if !seen.insert(entry.path.clone()) {
            continue;
        }
        fold_attributes(&entry.body, &mut macros, &mut pending);
    }
    macros
}

/// Walks `source` for `-define`, `-include`, and `-include_lib`. Other
/// attribute kinds are ignored. Bodies are kept verbatim.
fn fold_attributes(source: &str, macros: &mut MacroTable, pending: &mut Vec<PathBuf>) {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    let mut at_line_start = true;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\n' {
            at_line_start = true;
            i += 1;
            continue;
        }
        if c == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if at_line_start && c == b'-' {
            i += 1;
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &source[name_start..i];
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let body_start = i;
            i = scan_to_terminator(source, i);
            let body = &source[body_start..i];
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
            }
            match name {
                "define" => {
                    let trimmed = body
                        .trim()
                        .strip_prefix('(')
                        .map(|x| x.strip_suffix(')').unwrap_or(x))
                        .unwrap_or(body);
                    if let Some((key, value)) = parse_define(trimmed) {
                        macros.insert(key, value);
                    }
                }
                "include" | "include_lib" => {
                    if let Some(path) = parse_include_path(body) {
                        pending.push(path);
                    }
                }
                _ => {}
            }
            at_line_start = false;
            continue;
        }
        at_line_start = false;
        match c {
            b'"' => i = skip_string(source, i + 1),
            b'\'' => i = skip_atom(source, i + 1),
            _ => i += 1,
        }
    }
}

fn scan_to_terminator(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = start;
    let mut depth = 0i32;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth -= 1;
                i += 1;
            }
            b'.' if depth <= 0 => return i,
            b'"' => i = skip_string(source, i + 1),
            b'\'' => i = skip_atom(source, i + 1),
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    i
}

fn skip_string(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn skip_atom(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'\'' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn parse_include_path(body: &str) -> Option<PathBuf> {
    let s = body
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let s = s.strip_prefix('"')?;
    let s = s.strip_suffix('"')?;
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

/// Convenience wrapper: a flat map from path → file body, with two
/// resolution lookups (relative to the source file, then global).
#[derive(Debug, Default)]
pub struct FileMap {
    by_path: BTreeMap<PathBuf, String>,
    by_basename: BTreeMap<String, Vec<HeaderBody>>,
    by_include_lib: BTreeMap<String, HeaderBody>,
}

impl FileMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: PathBuf, body: String) {
        let base = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
        let lib_key = include_lib_key(&path);
        if let Some(base) = base {
            self.by_basename.entry(base).or_default().push(HeaderBody {
                path: path.clone(),
                body: body.clone(),
            });
        }
        if let Some(key) = lib_key {
            self.by_include_lib.insert(
                key,
                HeaderBody {
                    path: path.clone(),
                    body: body.clone(),
                },
            );
        }
        self.by_path.insert(path, body);
    }

    pub fn get(&self, path: &Path) -> Option<&str> {
        self.by_path.get(path).map(String::as_str)
    }
}

impl HeaderResolver for FileMap {
    fn resolve(&self, source_dir: Option<&Path>, raw: &Path) -> Option<HeaderBody> {
        if let Some(dir) = source_dir {
            let candidate = dir.join(raw);
            if let Some(body) = self.by_path.get(&candidate) {
                return Some(HeaderBody {
                    path: candidate,
                    body: body.clone(),
                });
            }
        }
        if let Some(body) = self.by_path.get(raw) {
            return Some(HeaderBody {
                path: raw.to_path_buf(),
                body: body.clone(),
            });
        }
        let key = raw.to_string_lossy();
        if let Some(entry) = self.by_include_lib.get(key.as_ref()) {
            return Some(entry.clone());
        }
        let base = raw.file_name().and_then(|n| n.to_str())?;
        self.by_basename.get(base).and_then(|v| v.first().cloned())
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
    let tail = rest.join("/");
    Some(format!("{app}/{tail}"))
}
