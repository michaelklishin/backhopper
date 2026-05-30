// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper xref backport-applicability` handler.
//!
//! Joins a TOML reference list of `(file, variant, functions[])` entries
//! against the target branch's snapshot `test_only_exports` and emits
//! per-row applicability verdicts.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;

use bel7_cli::{PARTIAL_SUCCESS_I32, TableStyle};
use serde::{Deserialize, Serialize};
use tabled::{Table, Tabled};

use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::{Module, Snapshot, TestExportVariant, state};
use backhopper_core::snapshot::parser;

use crate::cli::GlobalArgs;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};

#[derive(Debug, Deserialize)]
struct ReferenceFile {
    #[serde(default)]
    entry: Vec<ReferenceEntry>,
}

#[derive(Debug, Deserialize)]
struct ReferenceEntry {
    file: String,
    variant: ReferenceVariant,
    #[serde(default)]
    functions: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ReferenceVariant {
    A,
    B,
}

impl ReferenceVariant {
    fn to_test_export_variant(self) -> TestExportVariant {
        match self {
            Self::A => TestExportVariant::A,
            Self::B => TestExportVariant::B,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BackportApplicabilityPayload {
    pub rows: Vec<RowOutput>,
    pub totals: Totals,
}

#[derive(Debug, Serialize, Default)]
pub struct Totals {
    pub applies: usize,
    pub applies_with_adaptation: usize,
    pub n_a: usize,
}

#[derive(Debug, Serialize)]
pub struct RowOutput {
    pub file: String,
    pub module: Option<String>,
    pub reference_variant: &'static str,
    pub outcome: &'static str,
    pub detail: Option<String>,
}

pub fn handle(
    global: &GlobalArgs,
    reference_file_path: &Path,
    snapshot_file_path: &Path,
) -> CliResult<i32> {
    let ref_bytes = fs::read_to_string(reference_file_path).map_err(CliError::Io)?;
    let reference: ReferenceFile = toml::from_str(&ref_bytes)
        .map_err(|e| CliError::InvalidInput(format!("reference file parse error: {e}")))?;
    let snap_bytes = fs::read_to_string(snapshot_file_path).map_err(CliError::Io)?;
    let snapshot = parser::parse(&snap_bytes).map_err(|e| CliError::Core(e.into()))?;
    let payload = build_payload(&reference, &snapshot);
    let exit = backport_applicability_exit_code(
        payload.totals.applies_with_adaptation,
        payload.totals.n_a,
    );
    let ctx = OutputContext::new(global.formatter, "xref backport_applicability");
    let style = global.table_style;
    render_with_exit(&ctx, &payload, exit, |w| render_text(w, &payload, style))
}

// 0 only when every row is Applies; 3 when any row needs attention
pub fn backport_applicability_exit_code(
    applies_with_adaptation: usize,
    not_applicable: usize,
) -> i32 {
    if applies_with_adaptation == 0 && not_applicable == 0 {
        return 0;
    }
    PARTIAL_SUCCESS_I32
}

fn build_payload(
    reference: &ReferenceFile,
    snapshot: &Snapshot<state::Canonical>,
) -> BackportApplicabilityPayload {
    let mut rows = Vec::with_capacity(reference.entry.len());
    let mut totals = Totals::default();
    for entry in &reference.entry {
        let (module_name, outcome, detail) = classify_entry(entry, snapshot);
        let label = outcome.label();
        match outcome {
            Outcome::Applies => totals.applies += 1,
            Outcome::AppliesWithAdaptation => totals.applies_with_adaptation += 1,
            Outcome::NotApplicable => totals.n_a += 1,
        }
        rows.push(RowOutput {
            file: entry.file.clone(),
            module: module_name.map(|m| m.as_str().to_owned()),
            reference_variant: variant_label(entry.variant),
            outcome: label,
            detail,
        });
    }
    BackportApplicabilityPayload { rows, totals }
}

fn classify_entry(
    entry: &ReferenceEntry,
    snapshot: &Snapshot<state::Canonical>,
) -> (Option<ModuleName>, Outcome, Option<String>) {
    let Some(module_name) = module_name_from_path(&entry.file) else {
        return (
            None,
            Outcome::NotApplicable,
            Some(format!(
                "could not derive module name from {:?}",
                entry.file
            )),
        );
    };
    let Some(module) = snapshot.module_named(&module_name) else {
        return (
            Some(module_name),
            Outcome::NotApplicable,
            Some("module absent from target snapshot".to_owned()),
        );
    };
    let reference_functions = match parse_function_list(&entry.functions) {
        Ok(set) => set,
        Err(e) => {
            return (Some(module_name), Outcome::NotApplicable, Some(e));
        }
    };
    let pin_exports: BTreeSet<(FunctionName, Arity)> = module
        .test_only_exports
        .iter()
        .map(|t| (t.function.clone(), t.arity))
        .collect();
    if pin_exports.is_empty() {
        return (
            Some(module_name),
            Outcome::NotApplicable,
            Some("snapshot has no test_only_exports for this module".to_owned()),
        );
    }
    let reference_variant = entry.variant.to_test_export_variant();
    let variant_mismatches =
        count_variant_mismatches(module, &reference_functions, reference_variant);
    let set_only_in_reference: Vec<_> = reference_functions.difference(&pin_exports).collect();
    let set_only_in_pin: Vec<_> = pin_exports.difference(&reference_functions).collect();
    if set_only_in_reference.is_empty() && set_only_in_pin.is_empty() && variant_mismatches == 0 {
        return (Some(module_name), Outcome::Applies, None);
    }
    let mut notes = Vec::new();
    if !set_only_in_reference.is_empty() {
        notes.push(format!(
            "functions only in reference: {}",
            join_function_set(&set_only_in_reference)
        ));
    }
    if !set_only_in_pin.is_empty() {
        notes.push(format!(
            "functions only on branch: {}",
            join_function_set(&set_only_in_pin)
        ));
    }
    if variant_mismatches > 0 {
        notes.push(format!(
            "{variant_mismatches} function(s) have a different variant than the reference"
        ));
    }
    (
        Some(module_name),
        Outcome::AppliesWithAdaptation,
        Some(notes.join("; ")),
    )
}

fn module_name_from_path(path: &str) -> Option<ModuleName> {
    let basename = path.rsplit('/').next().unwrap_or(path);
    let stem = basename.strip_suffix(".erl")?;
    ModuleName::from_str(stem).ok()
}

fn parse_function_list(items: &[String]) -> Result<BTreeSet<(FunctionName, Arity)>, String> {
    let mut out = BTreeSet::new();
    for raw in items {
        let (name, arity) = raw
            .split_once('/')
            .ok_or_else(|| format!("expected name/arity in {raw:?}"))?;
        let name = FunctionName::from_str(name).map_err(|e| e.to_string())?;
        let arity = Arity::from_str(arity).map_err(|e| e.to_string())?;
        out.insert((name, arity));
    }
    Ok(out)
}

fn count_variant_mismatches(
    module: &Module,
    reference_functions: &BTreeSet<(FunctionName, Arity)>,
    reference_variant: TestExportVariant,
) -> usize {
    module
        .test_only_exports
        .iter()
        .filter(|t| t.variant != reference_variant)
        .filter(|t| {
            reference_functions
                .iter()
                .any(|(n, a)| n == &t.function && *a == t.arity)
        })
        .count()
}

fn join_function_set(items: &[&(FunctionName, Arity)]) -> String {
    let mut parts: Vec<String> = items.iter().map(|(n, a)| format!("{n}/{a}")).collect();
    parts.sort();
    parts.join(", ")
}

fn variant_label(v: ReferenceVariant) -> &'static str {
    match v {
        ReferenceVariant::A => "a",
        ReferenceVariant::B => "b",
    }
}

#[derive(Debug, Clone, Copy)]
enum Outcome {
    Applies,
    AppliesWithAdaptation,
    NotApplicable,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::Applies => "applies",
            Self::AppliesWithAdaptation => "applies-with-adaptation",
            Self::NotApplicable => "n_a",
        }
    }
}

#[derive(Tabled)]
struct DisplayRow {
    file: String,
    module: String,
    variant: String,
    outcome: String,
    detail: String,
}

fn render_text(
    w: &mut dyn Write,
    payload: &BackportApplicabilityPayload,
    style: TableStyle,
) -> CliResult<()> {
    writeln!(
        w,
        "applies: {}, applies-with-adaptation: {}, n_a: {}",
        payload.totals.applies, payload.totals.applies_with_adaptation, payload.totals.n_a,
    )?;
    if payload.rows.is_empty() {
        writeln!(w, "(no rows; reference list was empty)")?;
        return Ok(());
    }
    let rows: Vec<DisplayRow> = payload
        .rows
        .iter()
        .map(|r| DisplayRow {
            file: r.file.clone(),
            module: r.module.clone().unwrap_or_else(|| "-".into()),
            variant: r.reference_variant.to_owned(),
            outcome: r.outcome.to_owned(),
            detail: r.detail.clone().unwrap_or_default(),
        })
        .collect();
    let mut table = Table::new(rows);
    style.apply(&mut table);
    writeln!(w)?;
    writeln!(w, "{table}")?;
    Ok(())
}
