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
        self.cache
            .entry(suite_path.to_path_buf())
            .or_insert_with(|| fs::read_to_string(suite_path).unwrap_or_default())
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
    let needle = format!("{}:", module.as_str());
    let mut start = 0usize;
    while let Some(rel) = text[start..].find(&needle) {
        let pos = start + rel;
        let prev_ok = pos == 0
            || text
                .as_bytes()
                .get(pos.saturating_sub(1))
                .is_none_or(|&b| !is_atom_char(b as char));
        if prev_ok {
            return true;
        }
        start = pos + needle.len();
    }
    false
}

fn is_atom_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '@'
}
