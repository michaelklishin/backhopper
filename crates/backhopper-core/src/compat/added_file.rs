// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-`.erl` and per-`.hrl` 018 findings: runs the three resolvers
//! (test-module helpers, missing behaviours, missing headers) on a
//! single added file and returns the typed reasons plus the
//! diagnostic counters the caller merges into `Diagnostics`.
//!
//! The function is pure: no I/O beyond reading the supplied
//! `TargetTreeIndex`. The CLI is the orchestrator that decides which
//! `PatchedFile` shapes (fully added, modified, renamed) qualify.
//!
//! "Added file only" is the v1 scope: for a fully-added file the
//! post-patch source equals the concatenation of `HunkLine::Added`
//! lines. Modified files would need diff application against a
//! baseline read from the target tree, which is a follow-up.

use std::collections::BTreeMap;

use crate::compat::source_attributes::{
    behaviour_resolves, extract_behaviours, extract_includes, resolve_include,
};
use crate::compat::target_tree_index::TargetTreeIndex;
use crate::compat::test_suite::{TestSuiteFile, is_stdlib_or_otp};
use crate::model::names::{ModuleName, RelativePath};
use crate::model::verdict::{IncludeDirective, Reason};

#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddedFileFindings {
    pub reasons: Vec<Reason>,
    /// Per-suite `helper -> call_site_count` map, in the same shape
    /// as `Diagnostics.missing_test_modules`. Always-on counterpart of
    /// `Reason::TestModuleSymbolMissing`: emitted even when the
    /// target tree resolves the helper (in that case the inner map
    /// stays empty for that suite).
    pub missing_test_modules: BTreeMap<RelativePath, BTreeMap<ModuleName, usize>>,
}

impl AddedFileFindings {
    pub fn new(
        reasons: Vec<Reason>,
        missing_test_modules: BTreeMap<RelativePath, BTreeMap<ModuleName, usize>>,
    ) -> Self {
        Self {
            reasons,
            missing_test_modules,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.reasons.is_empty() && self.missing_test_modules.is_empty()
    }

    fn merge(&mut self, other: AddedFileFindings) {
        self.reasons.extend(other.reasons);
        for (suite, by_mod) in other.missing_test_modules {
            let entry = self.missing_test_modules.entry(suite).or_default();
            for (m, count) in by_mod {
                *entry.entry(m).or_insert(0) += count;
            }
        }
    }
}

/// Run the three 018 resolver passes on one added file and return
/// the merged findings. `path` identifies the file relative to the
/// repo root; `content` is the post-patch source.
pub fn analyse_added_file(
    path: &RelativePath,
    content: &str,
    target: &TargetTreeIndex,
    search_path_globs: &[String],
) -> AddedFileFindings {
    let mut out = AddedFileFindings::default();
    let is_erl = ends_with_ignore_case(path.as_str(), ".erl");
    let is_hrl = ends_with_ignore_case(path.as_str(), ".hrl");
    if !is_erl && !is_hrl {
        return out;
    }
    if is_erl && is_test_suite_path(path) {
        out.merge(analyse_test_suite(path, content, target, search_path_globs));
    }
    if is_erl {
        out.merge(analyse_behaviours(path, content, target, search_path_globs));
    }
    out.merge(analyse_includes(path, content, target));
    out
}

/// Run `analyse_added_file` for every entry in `files` and merge the
/// results into one envelope.
pub fn analyse_added_files<'a, I>(
    files: I,
    target: &TargetTreeIndex,
    search_path_globs: &[String],
) -> AddedFileFindings
where
    I: IntoIterator<Item = (&'a RelativePath, &'a str)>,
{
    let mut out = AddedFileFindings::default();
    for (path, content) in files {
        out.merge(analyse_added_file(path, content, target, search_path_globs));
    }
    out
}

fn analyse_test_suite(
    path: &RelativePath,
    content: &str,
    target: &TargetTreeIndex,
    search_path_globs: &[String],
) -> AddedFileFindings {
    let parsed = match TestSuiteFile::new(path.clone(), content.to_owned()).parse() {
        Ok(p) => p,
        Err(_) => return AddedFileFindings::default(),
    };
    let resolved = parsed.resolve(target, search_path_globs);
    let findings = resolved.into_findings();
    let mut missing = BTreeMap::new();
    if let Some((suite, by_mod)) = findings.diagnostic_entry {
        missing.insert(suite, by_mod);
    }
    AddedFileFindings {
        reasons: findings.reasons,
        missing_test_modules: missing,
    }
}

fn analyse_behaviours(
    path: &RelativePath,
    content: &str,
    target: &TargetTreeIndex,
    search_path_globs: &[String],
) -> AddedFileFindings {
    let mut reasons = Vec::new();
    for b in extract_behaviours(content) {
        if is_stdlib_or_otp(&b.behaviour) {
            continue;
        }
        if behaviour_resolves(target, search_path_globs, &b.behaviour) {
            continue;
        }
        reasons.push(Reason::BehaviourModuleMissing {
            source_path: path.clone(),
            behaviour: b.behaviour,
        });
    }
    AddedFileFindings {
        reasons,
        missing_test_modules: BTreeMap::new(),
    }
}

fn analyse_includes(
    path: &RelativePath,
    content: &str,
    target: &TargetTreeIndex,
) -> AddedFileFindings {
    let mut reasons = Vec::new();
    for inc in extract_includes(content) {
        if is_stdlib_include_lib(&inc.directive) {
            continue;
        }
        if let Err(attempted) = resolve_include(target, path, &inc.directive) {
            reasons.push(Reason::HeaderFileMissing {
                source_path: path.clone(),
                include_directive: inc.directive,
                attempted_paths: attempted,
            });
        }
    }
    AddedFileFindings {
        reasons,
        missing_test_modules: BTreeMap::new(),
    }
}

/// `-include_lib("kernel/include/file.hrl")` and friends live outside
/// the repo's target tree, so we never flag them missing even when
/// they fail to resolve under `deps/`, `apps/`, `lib/`. Only the
/// `IncludeLib` form gets this exemption: a `-include("…")` always
/// points at a project-local path.
pub fn is_stdlib_include_lib(directive: &IncludeDirective) -> bool {
    let path = match directive {
        IncludeDirective::IncludeLib { path } => path,
        IncludeDirective::Include { .. } => return false,
    };
    let app = path.split('/').next().unwrap_or("");
    matches!(
        app,
        "kernel"
            | "stdlib"
            | "compiler"
            | "common_test"
            | "ct"
            | "eunit"
            | "crypto"
            | "ssl"
            | "inets"
            | "public_key"
            | "asn1"
            | "snmp"
            | "tools"
            | "wx"
            | "xmerl"
            | "syntax_tools"
            | "edoc"
            | "debugger"
            | "diameter"
            | "erts"
            | "et"
            | "ftp"
            | "megaco"
            | "mnesia"
            | "observer"
            | "odbc"
            | "os_mon"
            | "parsetools"
            | "reltool"
            | "runtime_tools"
            | "sasl"
            | "ssh"
            | "tftp"
            | "proper"
            | "meck"
    )
}

fn is_test_suite_path(path: &RelativePath) -> bool {
    ends_with_ignore_case(path.as_str(), "_SUITE.erl")
}

fn ends_with_ignore_case(s: &str, suffix: &str) -> bool {
    let sb = s.as_bytes();
    let suf = suffix.as_bytes();
    sb.len() >= suf.len() && sb[sb.len() - suf.len()..].eq_ignore_ascii_case(suf)
}
