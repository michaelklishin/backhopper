//! AST-precise `SuiteMatcher`.
//!
//! Parses each suite via [`SourceReader`] and answers
//! "does this suite reference module M?" by looking at the parsed
//! `ModuleData`'s external calls, unresolved calls, and `-import`
//! declarations. Suites that fail to load (no `-module` attribute,
//! invalid UTF-8, parser bailout) fall through to a
//! [`SubstringMatcher`].
//!
//! Each suite file is parsed at most once and the result is cached
//! across calls.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backhopper_core::model::names::ModuleName;
use backhopper_core::suites::{SubstringMatcher, SuiteMatcher};
use backhopper_xref_graph::{CallTarget, FunctionRef};

use crate::model::ModuleData;
use crate::reader::SourceReader;

#[derive(Debug, Default)]
pub struct AstSuiteMatcher {
    reader: SourceReader,
    parsed: BTreeMap<PathBuf, ParsedSuite>,
    fallback: SubstringMatcher,
}

#[derive(Debug)]
enum ParsedSuite {
    Loaded(BTreeSet<ModuleName>),
    Failed,
}

impl AstSuiteMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the given suite was successfully parsed (its references
    /// came from the AST) or fell back to the substring matcher.
    pub fn parsed_cleanly(&self, suite_path: &Path) -> bool {
        matches!(self.parsed.get(suite_path), Some(ParsedSuite::Loaded(_)))
    }

    fn ensure_loaded(&mut self, suite_path: &Path) -> &ParsedSuite {
        if !self.parsed.contains_key(suite_path) {
            let parsed = load_suite_modules(&self.reader, suite_path);
            self.parsed.insert(suite_path.to_path_buf(), parsed);
        }
        &self.parsed[suite_path]
    }
}

impl SuiteMatcher for AstSuiteMatcher {
    fn modules_referenced_in_suite(
        &mut self,
        suite_path: &Path,
        triggering: &[ModuleName],
    ) -> BTreeSet<ModuleName> {
        match self.ensure_loaded(suite_path) {
            ParsedSuite::Loaded(refs) => triggering
                .iter()
                .filter(|m| refs.contains(*m))
                .cloned()
                .collect(),
            ParsedSuite::Failed => self
                .fallback
                .modules_referenced_in_suite(suite_path, triggering),
        }
    }
}

fn load_suite_modules(reader: &SourceReader, suite_path: &Path) -> ParsedSuite {
    let Ok(text) = fs::read_to_string(suite_path) else {
        return ParsedSuite::Failed;
    };
    let Ok((maybe_module, _warnings)) = reader.read_one(suite_path, &text) else {
        return ParsedSuite::Failed;
    };
    let Some(module) = maybe_module else {
        return ParsedSuite::Failed;
    };
    ParsedSuite::Loaded(collect_referenced_modules(&module))
}

/// Modules the suite refers to: every external call target, every
/// unresolved-function partial that names a module, every imported
/// module.
fn collect_referenced_modules(data: &ModuleData) -> BTreeSet<ModuleName> {
    let mut out: BTreeSet<ModuleName> = BTreeSet::new();
    for call in &data.external_calls {
        if let CallTarget::External(fn_ref) = &call.callee
            && let Some(m) = module_of(fn_ref)
        {
            out.insert(m.clone());
        }
    }
    for unresolved in &data.unresolved {
        if let Some(m) = module_of(&unresolved.partial) {
            out.insert(m.clone());
        }
    }
    for module in data.imports.keys() {
        out.insert(module.clone());
    }
    out
}

fn module_of(fn_ref: &FunctionRef) -> Option<&ModuleName> {
    match fn_ref {
        FunctionRef::Concrete(mfa) => Some(&mfa.module),
        FunctionRef::UnresolvedFunction { module, .. } => Some(module),
        _ => None,
    }
}
