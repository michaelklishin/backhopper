// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Types for suite selection.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app_src::AppSrcSpec;
use crate::model::names::{ApplicationName, ModuleName};

/// A test-suite module reference. `module` ends in `_SUITE` by
/// convention but the constructor does not enforce this.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SuiteRef {
    pub application: ApplicationName,
    pub module: ModuleName,
    pub path: PathBuf,
}

/// Why a suite was included in the plan. A suite can appear under
/// multiple reasons when more than one rule fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuiteInclusionReason {
    /// The suite file itself appears in the diff.
    TestModified { path: PathBuf },
    /// Fast unit or property suite swept because its application has
    /// modified source and the suite references at least one of the
    /// modified modules.
    UnitOrPropSweep {
        application: ApplicationName,
        triggering_modules: Vec<ModuleName>,
    },
    /// Suite is in the same application as a modified module and
    /// references that module by name.
    SameAppCaller {
        application: ApplicationName,
        module: ModuleName,
    },
    /// Suite is in a different application from a modified library
    /// module and references that module by name.
    CrossAppCaller {
        library_application: ApplicationName,
        module: ModuleName,
    },
    /// Suite was included by a user-configured rule.
    ConfiguredRule {
        rule_name: String,
        triggering_path: PathBuf,
    },
    /// A modified `.erl` is itself a behaviour module; this suite
    /// references one of its implementers.
    BehaviourImplementerSweep {
        behaviour: ModuleName,
        implementer: ModuleName,
    },
}

/// Plan input. The caller assembles diff, discovered apps, and any
/// extra rules from config.
#[derive(Debug, Clone, Default)]
pub struct PlanInput {
    pub repo_root: PathBuf,
    pub modified_paths: Vec<PathBuf>,
    pub apps: Vec<AppSrcSpec>,
    pub library_apps: Vec<ApplicationName>,
    pub extra_rules: Vec<ExtraRule>,
    /// Behaviour-to-implementers reverse map. When a modified `.erl`
    /// names a behaviour module, every implementer is pulled into scope
    /// and suites that reference them are added to the plan. Empty by
    /// default; populated from a snapshot when richer triage is wanted.
    pub implementer_index: BTreeMap<ModuleName, Vec<ModuleName>>,
    pub dep_module_index: BTreeMap<String, Vec<ModuleName>>,
}

/// One entry in the plan: the suite plus every rule that included it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuitePlanEntry {
    pub suite: SuiteRef,
    pub reasons: Vec<SuiteInclusionReason>,
}

/// Output of `plan()`. Entries are sorted by suite; reasons within
/// each entry are in rule-application order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuitePlan {
    pub entries: Vec<SuitePlanEntry>,
}

impl SuitePlan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// User-configured rule. Accepts both the literal `include_suites`
/// (application + module) and templated module names that resolve named
/// regex captures from the trigger pattern (e.g. `{plugin}_config_schema_SUITE`).
/// Templated includes look up the suite by module name across all
/// discovered applications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraRule {
    pub name: String,
    pub trigger: ExtraRuleTrigger,
    #[serde(default)]
    pub include_suites: Vec<SuiteRefSpec>,
    #[serde(default)]
    pub include_suite_templates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_match: Option<LineMatch>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_suite_for_dep_modules: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineMatch {
    pub pattern: String,
    #[serde(default)]
    pub captures: Vec<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// Path-glob trigger expression. `PathRegex` supports named captures
/// (`(?P<name>...)`) that templates referenced in
/// `ExtraRule::include_suite_templates` may substitute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum ExtraRuleTrigger {
    /// Fires when any modified path's file name matches `suffix`.
    PathSuffix { suffix: String },
    /// Fires when any modified path contains `fragment`.
    PathContains { fragment: String },
    /// Fires when any modified path matches `pattern`. The `captures`
    /// list enumerates the named capture groups templates may reference.
    /// Validated against the pattern at config-load time so unknown
    /// `{placeholder}`s fail fast instead of silently dropping at runtime.
    PathRegex {
        pattern: String,
        #[serde(default)]
        captures: Vec<String>,
    },
}

/// A suite to include when an `ExtraRule` fires. The caller resolves
/// these against the discovered tree at plan time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteRefSpec {
    pub application: ApplicationName,
    pub module: ModuleName,
}
