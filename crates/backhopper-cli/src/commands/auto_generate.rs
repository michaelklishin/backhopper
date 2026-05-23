// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use backhopper_core::Error as CoreError;
use backhopper_core::config::Config;
use backhopper_core::git::{GitRepo, version_cmp};
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::store::{Mutable, ReadOnly, SnapshotStore};

use crate::cli::GlobalArgs;
use crate::commands::context::open_store_mut;
use crate::commands::snapshots::build_snapshot;
use crate::errors::{CliError, CliResult};

pub fn missing_pins(store: &SnapshotStore<ReadOnly>, pins: &[Pin]) -> Vec<Pin> {
    pins.iter()
        .filter(|p| !store.has(&p.project, &p.tag))
        .cloned()
        .collect()
}

pub fn ensure_pin_snapshots_present(
    args: &GlobalArgs,
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    pins: &[Pin],
    auto_generate: bool,
) -> CliResult<()> {
    let missing = missing_pins(store, pins);
    if missing.is_empty() {
        return Ok(());
    }
    if !auto_generate {
        return Err(missing_snapshots_error(&missing));
    }
    let mut_store = open_store_mut(args, cfg)?;
    for pin in &missing {
        generate_one_pin(cfg, &mut_store, pin)?;
    }
    Ok(())
}

fn generate_one_pin(cfg: &Config, store: &SnapshotStore<Mutable>, pin: &Pin) -> CliResult<()> {
    let project = cfg
        .project(&pin.project)
        .map_err(|e| CliError::Core(e.into()))?;
    let repo = GitRepo::open(project.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let snapshot = build_snapshot(project, &repo, &pin.tag)?;
    store
        .write(&snapshot)
        .map_err(|e| CliError::Core(CoreError::Store(e)))?;
    Ok(())
}

pub fn missing_snapshots_error(missing: &[Pin]) -> CliError {
    let mut by_project: BTreeMap<&ProjectName, Vec<&TagName>> = BTreeMap::new();
    for pin in missing {
        by_project.entry(&pin.project).or_default().push(&pin.tag);
    }
    let mut commands: Vec<String> = Vec::new();
    for (project, tags) in &by_project {
        let oldest =
            oldest_version(tags).expect("by_project entries are non-empty by construction");
        commands.push(format!(
            "backhopper snapshots generate --project {project} --since {oldest}"
        ));
    }
    let remediation = if commands.len() == 1 {
        format!("run: {}", commands[0])
    } else {
        format!(
            "run:\n  {}\n  (or pass `--auto-generate` to do this inline)",
            commands.join("\n  ")
        )
    };
    CliError::MissingSnapshots {
        missing: missing.to_vec(),
        remediation,
    }
}

// Pick the version-oldest tag (not lex-min). For `version_cmp`, a tag
// that's numerically smaller compares as `Greater`, so `max_by` returns it.
fn oldest_version<'a>(tags: &[&'a TagName]) -> Option<&'a TagName> {
    tags.iter()
        .copied()
        .max_by(|a, b| version_cmp(a.as_str(), b.as_str()))
}
