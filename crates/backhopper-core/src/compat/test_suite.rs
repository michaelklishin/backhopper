// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `TestSuiteFile<S>` with the type-state pattern.
//!
//! The three states force the parse, then resolve, then read sequence
//! at compile time so a caller cannot read missing-module data from a
//! suite that has not been parsed or resolved against a target tree.
//!
//! Pipeline:
//!
//!  * `TestSuiteFile<Raw>::parse` → `TestSuiteFile<Parsed>`
//!  * `TestSuiteFile<Parsed>::resolve` → `TestSuiteFile<Resolved>`
//!  * only `TestSuiteFile<Resolved>` exposes `missing_modules` and
//!    `into_findings`

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str;

use thiserror::Error;

use backhopper_erlang_scan::{quoted_atom_span, skip_char_literal_span, string_span};

use crate::compat::otp::is_otp_module;
use crate::compat::target_tree_index::TargetTreeIndex;
use crate::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use crate::model::verdict::{Reason, TestCallSite};

pub mod state {
    mod sealed {
        pub trait Sealed {}
        impl Sealed for super::Raw {}
        impl Sealed for super::Parsed {}
        impl Sealed for super::Resolved {}
    }

    pub trait SuiteState: sealed::Sealed {}

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Raw;
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Parsed;
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Resolved;

    impl SuiteState for Raw {}
    impl SuiteState for Parsed {}
    impl SuiteState for Resolved {}
}

use state::{Parsed, Raw, Resolved, SuiteState};

/// One `Module:Function/Arity` reference found in the suite source.
/// Pure parser output: no judgement about whether `module` exists on
/// any target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperCall {
    pub module: ModuleName,
    pub function: FunctionName,
    pub arity: Arity,
    pub line: u32,
}

/// One unresolved helper module on the target, with the call sites
/// the suite uses to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingHelper {
    pub module: ModuleName,
    pub call_sites: Vec<TestCallSite>,
}

/// Reasons `TestSuiteFile::<Raw>::parse` may give up. None of these
/// abort verdict generation: the caller logs and drops the suite from
/// the missing-module pass.
#[non_exhaustive]
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("suite source is empty: {0}")]
    EmptySource(RelativePath),
}

/// Modules we never flag as missing even when absent from the target
/// tree. Two sources:
///
///   1. the existing OTP allowlist at `compat::otp::is_otp_module`,
///      shared with the `untracked_calls` diagnostic so the test-helper
///      resolver and the call-site classifier agree on what counts as
///      stdlib;
///   2. a small set of common-test, property-test, and mocking modules
///      outside OTP itself that are routine in RabbitMQ test SUITEs
///      (and never appear under `deps/<app>/test`).
pub fn is_stdlib_or_otp(module: &ModuleName) -> bool {
    if is_otp_module(module) {
        return true;
    }
    matches!(
        module.as_str(),
        "ct" | "ct_helper"
            | "ct_property_test"
            | "common_test"
            | "meck"
            | "proper"
            | "proper_types"
    )
}

#[derive(Debug, Clone)]
pub struct TestSuiteFile<S: SuiteState> {
    suite_path: RelativePath,
    source: String,
    calls: Vec<HelperCall>,
    missing: Vec<MissingHelper>,
    _state: PhantomData<S>,
}

impl TestSuiteFile<Raw> {
    pub fn new(suite_path: RelativePath, source: String) -> Self {
        Self {
            suite_path,
            source,
            calls: Vec::new(),
            missing: Vec::new(),
            _state: PhantomData,
        }
    }

    /// Scan the suite source and extract `Module:Function(...)`
    /// references where `Module` is a literal lowercase atom. Skips
    /// `?MODULE:`, `apply/3`, dispatch through a variable: those are
    /// counted in `Diagnostics.unanalyzed`, not here.
    pub fn parse(self) -> Result<TestSuiteFile<Parsed>, ParseError> {
        if self.source.trim().is_empty() {
            return Err(ParseError::EmptySource(self.suite_path));
        }
        let calls = scan_helper_calls(&self.source);
        Ok(TestSuiteFile {
            suite_path: self.suite_path,
            source: self.source,
            calls,
            missing: Vec::new(),
            _state: PhantomData,
        })
    }

    pub fn suite_path(&self) -> &RelativePath {
        &self.suite_path
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

impl TestSuiteFile<Parsed> {
    /// Distinct helper modules referenced by the suite, in
    /// first-occurrence order. Useful for the always-on diagnostic
    /// path (no target tree required).
    pub fn referenced_modules(&self) -> Vec<ModuleName> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for c in &self.calls {
            if seen.insert(c.module.clone()) {
                out.push(c.module.clone());
            }
        }
        out
    }

    pub fn calls(&self) -> &[HelperCall] {
        &self.calls
    }

    /// Resolve every referenced module against the target tree using
    /// the per-family `search_path_globs` (e.g. `deps/*/test` for
    /// `ProjectFamily::Rabbitmq`). Modules in `is_stdlib_or_otp` are
    /// dropped: they are outside the repo, so absence there means
    /// nothing.
    pub fn resolve(
        self,
        target: &TargetTreeIndex,
        search_path_globs: &[String],
    ) -> TestSuiteFile<Resolved> {
        let resolver = HelperResolver::new(target, search_path_globs);
        let mut by_module: BTreeMap<ModuleName, Vec<TestCallSite>> = BTreeMap::new();
        for call in &self.calls {
            if is_stdlib_or_otp(&call.module) {
                continue;
            }
            if resolver.resolves(&call.module) {
                continue;
            }
            by_module
                .entry(call.module.clone())
                .or_default()
                .push(TestCallSite {
                    function: call.function.clone(),
                    arity: call.arity,
                    line: call.line,
                });
        }
        let missing = by_module
            .into_iter()
            .map(|(module, call_sites)| MissingHelper { module, call_sites })
            .collect();
        TestSuiteFile {
            suite_path: self.suite_path,
            source: self.source,
            calls: self.calls,
            missing,
            _state: PhantomData,
        }
    }

    pub fn suite_path(&self) -> &RelativePath {
        &self.suite_path
    }
}

impl TestSuiteFile<Resolved> {
    pub fn suite_path(&self) -> &RelativePath {
        &self.suite_path
    }

    pub fn missing_modules(&self) -> &[MissingHelper] {
        &self.missing
    }

    /// Promote the missing helpers into both the typed verdict reasons
    /// and the diagnostic count map in one pass, without cloning the
    /// suite source or the parsed calls.
    pub fn into_findings(self) -> ResolvedFindings {
        if self.missing.is_empty() {
            return ResolvedFindings {
                reasons: Vec::new(),
                diagnostic_entry: None,
            };
        }
        let mut reasons = Vec::with_capacity(self.missing.len());
        let mut counts: BTreeMap<ModuleName, usize> = BTreeMap::new();
        for m in self.missing {
            counts.insert(m.module.clone(), m.call_sites.len());
            reasons.push(Reason::TestModuleSymbolMissing {
                suite_path: self.suite_path.clone(),
                missing_module: m.module,
                call_sites: m.call_sites,
            });
        }
        ResolvedFindings {
            reasons,
            diagnostic_entry: Some((self.suite_path, counts)),
        }
    }
}

/// Combined verdict and diagnostic output of one resolved suite.
/// Returned by `TestSuiteFile::<Resolved>::into_findings` so callers
/// get both surfaces without cloning data that is then discarded.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ResolvedFindings {
    pub reasons: Vec<Reason>,
    pub diagnostic_entry: Option<(RelativePath, BTreeMap<ModuleName, usize>)>,
}

/// Scan the source character by character, balancing `()`, `[]`, `{}`,
/// `''`, `""`, and `%` line comments. Records `module:function(`
/// references where `module` is a literal lowercase atom. The scanner
/// is conservative: anything it cannot disambiguate
/// (dispatch through a variable, `?MODULE:f`, `apply/3`) is dropped
/// rather than guessed.
fn scan_helper_calls(source: &str) -> Vec<HelperCall> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if b == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'"' || b == b'~' {
            let span = string_span(bytes, i).unwrap_or(1);
            line += newline_count(&bytes[i..i + span]);
            i += span;
            continue;
        }
        if b == b'$' {
            let span = skip_char_literal_span(bytes, i);
            line += newline_count(&bytes[i..i + span]);
            i += span;
            continue;
        }
        if b == b'\'' {
            let span = quoted_atom_span(bytes, i).unwrap_or(1);
            line += newline_count(&bytes[i..i + span]);
            i += span;
            continue;
        }
        // skip the whole variable span so a restart inside Mod does not read od:f(...) as a call
        if b.is_ascii_uppercase() || b == b'_' {
            while i < bytes.len() && is_atom_continue(bytes[i]) {
                i += 1;
            }
            continue;
        }
        // skip a ?MACRO whole so ?MODULE:f(...) does not surface module:f(...)
        if b == b'?' {
            i += 1;
            while i < bytes.len() && is_atom_continue(bytes[i]) {
                i += 1;
            }
            continue;
        }
        if is_atom_start(b) {
            let start = i;
            while i < bytes.len() && is_atom_continue(bytes[i]) {
                i += 1;
            }
            let atom = str::from_utf8(&bytes[start..i]).unwrap_or("");
            if i < bytes.len() && bytes[i] == b':' {
                let after_colon = i + 1;
                let mut j = after_colon;
                while j < bytes.len() && is_atom_continue(bytes[j]) {
                    j += 1;
                }
                if j > after_colon {
                    let func = str::from_utf8(&bytes[after_colon..j]).unwrap_or("");
                    if !func.starts_with(|c: char| c.is_ascii_lowercase()) {
                        i = j;
                        continue;
                    }
                    let lookahead = skip_spaces(bytes, j);
                    if lookahead < bytes.len() && bytes[lookahead] == b'(' {
                        let arity = count_top_level_args(bytes, lookahead);
                        if let Ok(arity_u8) = u8::try_from(arity)
                            && let (Ok(module), Ok(function)) =
                                (ModuleName::new(atom), FunctionName::new(func))
                        {
                            out.push(HelperCall {
                                module,
                                function,
                                arity: Arity::new(arity_u8),
                                line,
                            });
                        }
                        i = lookahead;
                        continue;
                    }
                }
                i = j;
                continue;
            }
            continue;
        }
        i += 1;
    }
    out
}

fn is_atom_start(b: u8) -> bool {
    b.is_ascii_lowercase()
}

fn is_atom_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

fn newline_count(span: &[u8]) -> u32 {
    let mut n = 0;
    for &b in span {
        if b == b'\n' {
            n += 1;
        }
    }
    n
}

fn skip_spaces(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

/// Count comma-separated top-level arguments inside the parenthesised
/// group starting at `bytes[open]` (which must be `b'('`). A call with
/// no arguments returns 0. Nested brackets and strings are skipped.
fn count_top_level_args(bytes: &[u8], open: usize) -> u32 {
    debug_assert_eq!(bytes[open], b'(');
    let mut depth = 1i32;
    let mut i = open + 1;
    let mut saw_token = false;
    let mut commas = 0u32;
    while i < bytes.len() && depth > 0 {
        let b = bytes[i];
        match b {
            b'(' | b'[' | b'{' => {
                depth += 1;
                saw_token = true;
            }
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b',' if depth == 1 => {
                commas += 1;
            }
            b'"' | b'~' => {
                i += string_span(bytes, i).unwrap_or(1);
                saw_token = true;
                continue;
            }
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                saw_token = true;
                continue;
            }
            b'\'' => {
                i += quoted_atom_span(bytes, i).unwrap_or(1);
                saw_token = true;
                continue;
            }
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b if !b.is_ascii_whitespace() => {
                saw_token = true;
            }
            _ => {}
        }
        i += 1;
    }
    if saw_token { commas + 1 } else { 0 }
}

/// Resolve a single module against the same search-path globs the
/// SUITE helper resolver uses. Exposed for the source-attribute
/// resolvers (`behaviour_resolves` in `source_attributes`) so the two
/// surfaces share one glob-expansion implementation.
pub fn module_resolves(
    target: &TargetTreeIndex,
    search_path_globs: &[String],
    module: &ModuleName,
) -> bool {
    HelperResolver::new(target, search_path_globs).resolves(module)
}

/// Compile per-family glob set into a closed list of
/// `(prefix_components_count, prefix_components, suffix_components)`
/// triples. At query time the resolver tests each candidate file path
/// against each triple: prefix matched component-wise, exactly one
/// wildcard segment in the middle, suffix matched, filename equals
/// `<module>.erl`. Linear in the number of present paths on the
/// target, which the `TargetTreeIndex` already pre-computes.
struct HelperResolver<'a> {
    target: &'a TargetTreeIndex,
    literal_roots: Vec<PathBuf>,
    wildcard_roots: Vec<WildcardRoot>,
}

#[derive(Debug, Clone)]
struct WildcardRoot {
    prefix: PathBuf,
    suffix: PathBuf,
}

impl<'a> HelperResolver<'a> {
    fn new(target: &'a TargetTreeIndex, search_path_globs: &[String]) -> Self {
        let mut literal_roots = Vec::new();
        let mut wildcard_roots = Vec::new();
        for g in search_path_globs {
            match split_single_wildcard(g) {
                Some((prefix, suffix)) => wildcard_roots.push(WildcardRoot { prefix, suffix }),
                None => literal_roots.push(PathBuf::from(g)),
            }
        }
        Self {
            target,
            literal_roots,
            wildcard_roots,
        }
    }

    /// True when `<module>.erl` is present at any literal search root
    /// or matches any wildcard root on the target tree.
    fn resolves(&self, module: &ModuleName) -> bool {
        let filename = format!("{}.erl", module.as_str());
        for root in &self.literal_roots {
            if self.target.contains_path(&root.join(&filename)) {
                return true;
            }
        }
        if self.wildcard_roots.is_empty() {
            return false;
        }
        // fast path: match the module's one indexed .erl file instead of scanning every path
        if let Some(path) = self.target.module_erl_path(module) {
            return self
                .wildcard_roots
                .iter()
                .any(|w| path_matches_wildcard(path, &w.prefix, &w.suffix));
        }
        // on absence or a basename clash, fall back to the linear scan so a clashing file stays matchable
        for path in self.target.present_paths() {
            if path.file_name().and_then(|s| s.to_str()) != Some(filename.as_str()) {
                continue;
            }
            for w in &self.wildcard_roots {
                if path_matches_wildcard(path, &w.prefix, &w.suffix) {
                    return true;
                }
            }
        }
        false
    }
}

/// Splits a glob like `deps/*/test` into `(deps, test)` (the segments
/// before and after the single `*`). Returns `None` when the glob has
/// no wildcard or has more than one: we only support the single-`*`
/// form `<prefix>/*/<suffix>`.
fn split_single_wildcard(glob: &str) -> Option<(PathBuf, PathBuf)> {
    let segs: Vec<&str> = glob.split('/').collect();
    let mut star_at = None;
    for (i, s) in segs.iter().enumerate() {
        if s.contains('*') {
            if *s != "*" || star_at.is_some() {
                return None;
            }
            star_at = Some(i);
        }
    }
    let star_at = star_at?;
    let prefix: PathBuf = segs[..star_at].iter().collect();
    let suffix: PathBuf = segs[star_at + 1..].iter().collect();
    Some((prefix, suffix))
}

/// `path` matches `<prefix>/<one-segment>/<suffix>/<filename>`. The
/// caller has already verified the filename; this checks the
/// containing directory.
fn path_matches_wildcard(path: &Path, prefix: &Path, suffix: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let parent_comps: Vec<_> = parent.components().collect();
    let prefix_comps: Vec<_> = prefix.components().collect();
    let suffix_comps: Vec<_> = suffix.components().collect();
    if parent_comps.len() != prefix_comps.len() + 1 + suffix_comps.len() {
        return false;
    }
    if parent_comps[..prefix_comps.len()] != prefix_comps[..] {
        return false;
    }
    let suffix_start = prefix_comps.len() + 1;
    parent_comps[suffix_start..] == suffix_comps[..]
}
