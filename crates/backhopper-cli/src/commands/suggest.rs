// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use serde::Serialize;
use toml_edit::{Array, DocumentMut, Item, Table, value};

use backhopper_core::compat::is_otp_module;
use backhopper_core::model::names::{ModuleName, ProjectName};
use backhopper_core::model::verdict::Diagnostics;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectSuggestion {
    pub name: String,
    pub modules: Vec<String>,
    pub call_sites: usize,
}

pub fn build_suggestions(
    diagnostics: &Diagnostics,
    known_projects: &[ProjectName],
) -> Vec<ProjectSuggestion> {
    let known: BTreeSet<&str> = known_projects.iter().map(ProjectName::as_str).collect();
    let mut bucket: BTreeMap<String, ProjectSuggestion> = BTreeMap::new();
    for (module, count) in &diagnostics.untracked_calls {
        if is_otp_module(module) {
            continue;
        }
        let Some(prefix) = infer_prefix(module) else {
            continue;
        };
        if known.contains(prefix) {
            continue;
        }
        let entry = bucket
            .entry(prefix.to_owned())
            .or_insert_with(|| ProjectSuggestion {
                name: prefix.to_owned(),
                modules: Vec::new(),
                call_sites: 0,
            });
        entry.modules.push(module.to_string());
        entry.call_sites += count;
    }
    let mut out: Vec<ProjectSuggestion> = bucket.into_values().collect();
    for s in &mut out {
        s.modules.sort();
    }
    out.sort_by(|a, b| {
        b.call_sites
            .cmp(&a.call_sites)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

// Substring before the first underscore. A bare `cowboy` (no underscore) is
// not a useful suggestion, so we drop it.
fn infer_prefix(module: &ModuleName) -> Option<&str> {
    module.as_str().split_once('_').map(|(p, _)| p)
}

pub fn render_suggestion<W: Write + ?Sized>(
    w: &mut W,
    s: &ProjectSuggestion,
    module_preview_limit: usize,
) -> io::Result<()> {
    let plural = if s.call_sites == 1 { "" } else { "s" };
    writeln!(
        w,
        "# {} untracked call{} would become tracked if this project is added",
        s.call_sites, plural,
    )?;
    let shown: Vec<&str> = s
        .modules
        .iter()
        .take(module_preview_limit)
        .map(String::as_str)
        .collect();
    let rest = s.modules.len().saturating_sub(shown.len());
    if rest == 0 {
        writeln!(w, "# Modules: {}", shown.join(", "))?;
    } else {
        writeln!(w, "# Modules: {} (+{} more)", shown.join(", "), rest)?;
    }
    writeln!(w, "[[project]]")?;
    writeln!(w, "name       = \"{}\"", s.name)?;
    writeln!(w, "language   = \"erlang\"")?;
    writeln!(w, "git_url    = \"TODO: set to a clone URL or local path\"")?;
    writeln!(w, "tag_prefix = \"v\"")?;
    Ok(())
}

pub fn append_suggestions_to_config(
    config_text: &str,
    suggestions: &[ProjectSuggestion],
) -> Result<String, String> {
    let mut doc: DocumentMut = config_text
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("invalid TOML in config: {e}"))?;
    let existing_names = collect_existing_project_names(&doc);
    let projects_array = doc
        .entry("project")
        .or_insert(Item::ArrayOfTables(Default::default()))
        .as_array_of_tables_mut()
        .ok_or_else(|| "config 'project' key is not an array of tables".to_owned())?;
    for s in suggestions {
        if existing_names.contains(s.name.as_str()) {
            continue;
        }
        projects_array.push(build_project_stub(s));
    }
    Ok(doc.to_string())
}

fn collect_existing_project_names(doc: &DocumentMut) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(arr) = doc.get("project").and_then(Item::as_array_of_tables) {
        for table in arr {
            if let Some(name) = table.get("name").and_then(|v| v.as_str()) {
                out.insert(name.to_owned());
            }
        }
    }
    out
}

fn build_project_stub(s: &ProjectSuggestion) -> Table {
    let mut table = Table::new();
    table["name"] = value(s.name.as_str());
    table["language"] = value("erlang");
    table["git_url"] = value("TODO: set to a clone URL or local path");
    table["tag_prefix"] = value("v");
    let mut modules = Array::new();
    for m in s.modules.iter().take(8) {
        modules.push(m.as_str());
    }
    table["public_modules"] = value(modules);
    table
}
