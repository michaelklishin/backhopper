// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `InapplicableReason`, `TouchedKinds`, and `FileKind`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::names::{ProjectName, RelativePath};

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "reason", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum InapplicableReason {
    NoErlangSurfaceTouched,
    OnlyDocsTouched,
    OnlyTestFixturesTouched,
    OnlySchemaTouched,
    OnlyMakefileTouched,
    OnlyMixExsTouched,
    OnlyCiWorkflowTouched,
    OnlyAppSrcTouched,
    OnlyRebarConfigTouched,
    /// Every changed `.erl` hunk is a Variant A unwrap: dropping
    /// `-ifdef(TEST).` or `-endif.` around an `-export(...)` directive,
    /// or rewriting `-compile(export_all).` to an explicit
    /// `-export([...])`. No call sites, no specs, no bodies touched:
    /// only test-visibility metadata.
    OnlyTestVisibilityChanged,
    /// The patch touches Erlang surface but every referenced symbol
    /// resolves to the self-pin or to a local definition: every
    /// tracked-dep pin has zero in-scope references. Without this
    /// reason the operator would see many vacuous `Compatible`
    /// rows for a patch that backhopper genuinely has no signal on.
    OnlySelfSurfaceTouched,
    /// Every touched path is in a sibling project's scope, not this
    /// pin's. The pin has nothing to say about the patch. `project`
    /// names the first sibling project (alphabetical) that owns the
    /// out-of-scope paths.
    OutOfScopeFor {
        project: ProjectName,
    },
    /// Every touched path is unclaimed by any configured project's
    /// scan_paths or app_roots. Vendored files, docs, build scripts.
    Untracked,
    /// `--target-repo-dir-path` was supplied and every touched path
    /// is missing from the target tree, with no configured
    /// translation mapping any of them. The patch as written does
    /// not apply to the target.
    PathsMissingOnTarget {
        paths: Vec<RelativePath>,
    },
}

impl InapplicableReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoErlangSurfaceTouched => "no_erlang_surface_touched",
            Self::OnlyDocsTouched => "only_docs_touched",
            Self::OnlyTestFixturesTouched => "only_test_fixtures_touched",
            Self::OnlySchemaTouched => "only_schema_touched",
            Self::OnlyMakefileTouched => "only_makefile_touched",
            Self::OnlyMixExsTouched => "only_mix_exs_touched",
            Self::OnlyCiWorkflowTouched => "only_ci_workflow_touched",
            Self::OnlyAppSrcTouched => "only_app_src_touched",
            Self::OnlyRebarConfigTouched => "only_rebar_config_touched",
            Self::OnlyTestVisibilityChanged => "only_test_visibility_changed",
            Self::OnlySelfSurfaceTouched => "only_self_surface_touched",
            Self::OutOfScopeFor { .. } => "out_of_scope_for",
            Self::Untracked => "untracked",
            Self::PathsMissingOnTarget { .. } => "paths_missing_on_target",
        }
    }
}

/// Per-pin tally of file kinds the patch touched. Lets `promote_inapplicable`
/// tell a real green check from a diff with nothing analyzable to check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TouchedKinds {
    #[serde(default)]
    pub erl: u32,
    #[serde(default)]
    pub hrl: u32,
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub docs: u32,
    #[serde(default)]
    pub tests: u32,
    #[serde(default)]
    pub makefile: u32,
    #[serde(default)]
    pub mix_exs: u32,
    #[serde(default)]
    pub ci_workflow: u32,
    #[serde(default)]
    pub app_src: u32,
    #[serde(default)]
    pub rebar_config: u32,
    #[serde(default)]
    pub other: u32,
    /// Every changed `.erl` hunk is a Variant A unwrap of test-only
    /// `-ifdef(TEST)` guards around an `-export` directive (see
    /// `InapplicableReason::OnlyTestVisibilityChanged`). When true,
    /// the patch touches Erlang surface but adds no semantic
    /// content, so an empty reason set should promote to
    /// `Inapplicable` instead of `Compatible`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_test_visibility: bool,
    /// The patch touches Erlang surface but references no tracked-dep
    /// API (every non-self pin has zero in-scope references). The CLI
    /// sets this after `evaluate_series` when a self-pin is present
    /// and every non-self pin's `tracked_refs` is empty; see
    /// `InapplicableReason::OnlySelfSurfaceTouched`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_self_surface: bool,
}

// &bool is required by serde's skip_serializing_if predicate shape.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

impl TouchedKinds {
    pub fn is_empty(&self) -> bool {
        // Destructure so adding a new field forces this method to be updated.
        let Self {
            erl,
            hrl,
            schema,
            docs,
            tests,
            makefile,
            mix_exs,
            ci_workflow,
            app_src,
            rebar_config,
            other,
            only_test_visibility,
            only_self_surface,
        } = self;
        *erl == 0
            && *hrl == 0
            && *schema == 0
            && *docs == 0
            && *tests == 0
            && *makefile == 0
            && *mix_exs == 0
            && *ci_workflow == 0
            && *app_src == 0
            && *rebar_config == 0
            && *other == 0
            && !*only_test_visibility
            && !*only_self_surface
    }

    pub fn classify(path: &Path) -> FileKind {
        let p = path.to_string_lossy();
        let lower = p.to_ascii_lowercase();
        let is_erl_source = lower.ends_with(".erl") || lower.ends_with(".hrl");
        // a docs path component must not reclassify real Erlang source as docs
        if lower.ends_with(".md")
            || lower.ends_with(".adoc")
            || lower.ends_with(".rst")
            || lower.ends_with(".txt")
            || (!is_erl_source && (lower.contains("/docs/") || lower.starts_with("docs/")))
        {
            return FileKind::Docs;
        }
        if lower.contains("_suite_data/")
            || lower.contains("/test/")
            || lower.contains("/tests/")
            || lower.starts_with("test/")
            || lower.starts_with("tests/")
            || lower.ends_with("_suite.erl")
        {
            return FileKind::Tests;
        }
        if lower.ends_with(".schema") || lower.ends_with(".snippets") {
            return FileKind::Schema;
        }
        if lower.ends_with(".erl") {
            return FileKind::Erl;
        }
        if lower.ends_with(".hrl") {
            return FileKind::Hrl;
        }
        if lower.ends_with(".app.src") || lower.ends_with(".app.src.script") {
            return FileKind::AppSrc;
        }
        if lower.ends_with("rebar.config") || lower.ends_with("rebar3.config") {
            return FileKind::RebarConfig;
        }
        if lower.ends_with("/mix.exs")
            || lower == "mix.exs"
            || lower.ends_with("/mix.lock")
            || lower == "mix.lock"
        {
            return FileKind::MixExs;
        }
        if Self::is_ci_workflow(&lower) {
            return FileKind::CiWorkflow;
        }
        if Self::is_makefile(&lower) {
            return FileKind::Makefile;
        }
        FileKind::Other
    }

    fn is_ci_workflow(lower: &str) -> bool {
        if lower.contains(".github/workflows/") {
            return lower.ends_with(".yml") || lower.ends_with(".yaml");
        }
        lower.contains(".github/actions/")
    }

    fn is_makefile(lower: &str) -> bool {
        if lower.ends_with(".mk") {
            return true;
        }
        let basename = lower.rsplit('/').next().unwrap_or(lower);
        matches!(
            basename,
            "makefile" | "erlang.mk" | "rabbitmq-components.mk"
        )
    }

    pub fn record(&mut self, kind: FileKind) {
        match kind {
            FileKind::Erl => self.erl += 1,
            FileKind::Hrl => self.hrl += 1,
            FileKind::Schema => self.schema += 1,
            FileKind::Docs => self.docs += 1,
            FileKind::Tests => self.tests += 1,
            FileKind::Makefile => self.makefile += 1,
            FileKind::MixExs => self.mix_exs += 1,
            FileKind::CiWorkflow => self.ci_workflow += 1,
            FileKind::AppSrc => self.app_src += 1,
            FileKind::RebarConfig => self.rebar_config += 1,
            FileKind::Other => self.other += 1,
        }
    }

    pub fn from_paths<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut tk = Self::default();
        for p in paths {
            tk.record(Self::classify(p.as_ref()));
        }
        tk
    }

    /// `None` when any `.erl` or `.hrl` was touched: an analyzable diff with
    /// zero reasons is real `Compatible`, not `Inapplicable`.
    pub fn inapplicable_reason(&self) -> Option<InapplicableReason> {
        if self.only_test_visibility {
            return Some(InapplicableReason::OnlyTestVisibilityChanged);
        }
        if self.only_self_surface {
            return Some(InapplicableReason::OnlySelfSurfaceTouched);
        }
        // .schema files carry MFA references in their fun bodies, so a
        // schema-touching diff is analyzable surface, not inapplicable.
        if self.erl > 0 || self.hrl > 0 || self.schema > 0 {
            return None;
        }
        if self.is_empty() {
            return Some(InapplicableReason::NoErlangSurfaceTouched);
        }
        if self.is_only(|tk| tk.docs > 0) {
            return Some(InapplicableReason::OnlyDocsTouched);
        }
        if self.is_only(|tk| tk.tests > 0) {
            return Some(InapplicableReason::OnlyTestFixturesTouched);
        }
        if self.is_only(|tk| tk.makefile > 0) {
            return Some(InapplicableReason::OnlyMakefileTouched);
        }
        if self.is_only(|tk| tk.mix_exs > 0) {
            return Some(InapplicableReason::OnlyMixExsTouched);
        }
        if self.is_only(|tk| tk.ci_workflow > 0) {
            return Some(InapplicableReason::OnlyCiWorkflowTouched);
        }
        if self.is_only(|tk| tk.app_src > 0) {
            return Some(InapplicableReason::OnlyAppSrcTouched);
        }
        if self.is_only(|tk| tk.rebar_config > 0) {
            return Some(InapplicableReason::OnlyRebarConfigTouched);
        }
        Some(InapplicableReason::NoErlangSurfaceTouched)
    }

    /// True when exactly the bucket selected by `selector` is non-zero
    /// and every other (non-Erlang-surface) bucket is zero.
    fn is_only<F: Fn(&Self) -> bool>(&self, selector: F) -> bool {
        if !selector(self) {
            return false;
        }
        let buckets = [
            self.docs > 0,
            self.tests > 0,
            self.makefile > 0,
            self.mix_exs > 0,
            self.ci_workflow > 0,
            self.app_src > 0,
            self.rebar_config > 0,
            self.other > 0,
        ];
        let active = buckets.iter().filter(|b| **b).count();
        active == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Erl,
    Hrl,
    Schema,
    Docs,
    Tests,
    Makefile,
    MixExs,
    CiWorkflow,
    AppSrc,
    RebarConfig,
    Other,
}
