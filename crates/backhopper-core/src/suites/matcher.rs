// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Pluggable strategy for "does this suite reference module M?".
//!
//! The built-in [`SubstringMatcher`] does a cheap byte-level substring
//! search with an atom-character boundary check (`M:` with the byte
//! before, if any, outside `[a-zA-Z0-9_@]`). It over-includes through
//! comments and string literals but never silently misses a real
//! reference: false positives are cheaper than false negatives.
//!
//! A more precise implementation lives in `backhopper-xref-reader`
//! (`AstSuiteMatcher`): it parses each suite into a `ModuleData` and
//! consults `external_calls`, `unresolved`, and `imports` for actual
//! references. Suites that fail to load fall back to the substring
//! matcher.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::names::ModuleName;

/// Returns the subset of `triggering` modules referenced by `suite_path`.
/// Implementations may cache parsed content across calls; the trait is
/// `&mut self` for that reason.
pub trait SuiteMatcher {
    fn modules_referenced_in_suite(
        &mut self,
        suite_path: &Path,
        triggering: &[ModuleName],
    ) -> BTreeSet<ModuleName>;
}

/// Substring matcher with a per-path file-content cache.
#[derive(Debug, Default)]
pub struct SubstringMatcher {
    cache: BTreeMap<PathBuf, String>,
}

impl SubstringMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    fn cached_text(&mut self, suite_path: &Path) -> &str {
        if !self.cache.contains_key(suite_path) {
            let text = fs::read_to_string(suite_path).unwrap_or_default();
            self.cache.insert(suite_path.to_path_buf(), text);
        }
        self.cache
            .get(suite_path)
            .map(String::as_str)
            .unwrap_or_default()
    }
}

impl SuiteMatcher for SubstringMatcher {
    fn modules_referenced_in_suite(
        &mut self,
        suite_path: &Path,
        triggering: &[ModuleName],
    ) -> BTreeSet<ModuleName> {
        let text = self.cached_text(suite_path);
        triggering
            .iter()
            .filter(|m| text_references_module(text, m))
            .cloned()
            .collect()
    }
}

/// Whether `text` references `module` as `module:` somewhere. The byte
/// before each match must not be part of an atom identifier, so
/// `not_rabbit_db:` does not match `rabbit_db`.
pub fn text_references_module(text: &str, module: &ModuleName) -> bool {
    text_references_named_module(text, module.as_str())
}

pub(crate) fn text_references_named_module(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = text[start..].find(name) {
        let pos = start + rel;
        if bytes.get(pos + name.len()) == Some(&b':') && (pos == 0 || !is_atom_byte(bytes[pos - 1]))
        {
            return true;
        }
        start = pos + name.len();
    }
    false
}

fn is_atom_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}