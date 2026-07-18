// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use backhopper_core::config::{Config, Language};
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::store::{Mutable, ReadOnly, SnapshotStore};
use backhopper_core::versions::version_cmp;

use crate::cli::GlobalArgs;
use crate::commands::context::{open_project_repo, open_store_mut};
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

pub fn generate_one_pin(cfg: &Config, store: &SnapshotStore<Mutable>, pin: &Pin) -> CliResult<()> {
    let project = cfg.project(&pin.project)?;
    let repo = open_project_repo(project)?;
    let snapshot = build_snapshot(project, &repo, &pin.tag)?;
    store.write(&snapshot)?;
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
        commands.push(snapshot_generate_command(project, oldest));
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

/// The one remedy phrasing for a missing snapshot, shared by check
/// errors, pin-bump notes, and doctor so it cannot drift.
pub fn snapshot_generate_command(project: &ProjectName, since: &TagName) -> String {
    format!("backhopper snapshots generate --project {project} --since {since}")
}

// pick the version-oldest tag: version_cmp is reversed, so max_by returns the smallest version
fn oldest_version<'a>(tags: &[&'a TagName]) -> Option<&'a TagName> {
    tags.iter()
        .copied()
        .max_by(|a, b| version_cmp(a.as_str(), b.as_str()))
}

/// Per-pin status from a non-mutating coverage pre-flight. Named `Row`
/// to keep it distinct from `pin_coverage::PinCoverage`, a different
/// enum one module over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinCoverageRow {
    pub pin: Pin,
    pub status: PinCoverageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinCoverageStatus {
    Present,
    Missing,
    StaleExtractor { stored: String, expected: String },
}

/// Freshness of one pin's on-disk snapshot against the running
/// extractor. The single classifier both `coverage_report` and `repos
/// doctor` read, so their present, missing, and stale verdicts cannot
/// drift apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotFreshness {
    Present,
    Missing,
    Stale { stored: String, expected: String },
}

pub(crate) fn classify_snapshot_freshness(
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    pin: &Pin,
) -> SnapshotFreshness {
    if !store.has(&pin.project, &pin.tag) {
        return SnapshotFreshness::Missing;
    }
    let expected = expected_extractor_version(cfg, &pin.project);
    match store.read(&pin.project, &pin.tag) {
        Ok(snap) => {
            let stored = snap.header().extractor_version.as_str();
            if !stored.is_empty() && stored != expected {
                SnapshotFreshness::Stale {
                    stored: stored.to_owned(),
                    expected: expected.to_owned(),
                }
            } else {
                SnapshotFreshness::Present
            }
        }
        // a read failure of an on-disk file counts as missing for pre-flight: the user must regenerate
        Err(_) => SnapshotFreshness::Missing,
    }
}

/// Walk each pin once and classify: present, missing on disk, or
/// present-but-stale relative to the running extractor version. Pure read
/// path: never writes, never auto-generates.
pub fn coverage_report(
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    pins: &[Pin],
) -> Vec<PinCoverageRow> {
    pins.iter()
        .map(|pin| {
            let status = match classify_snapshot_freshness(cfg, store, pin) {
                SnapshotFreshness::Present => PinCoverageStatus::Present,
                SnapshotFreshness::Missing => PinCoverageStatus::Missing,
                SnapshotFreshness::Stale { stored, expected } => {
                    PinCoverageStatus::StaleExtractor { stored, expected }
                }
            };
            PinCoverageRow {
                pin: pin.clone(),
                status,
            }
        })
        .collect()
}

pub(crate) fn expected_extractor_version(cfg: &Config, project: &ProjectName) -> &'static str {
    match cfg.project(project).map(|p| p.language) {
        Ok(Language::Erlang) => backhopper_erlang::EXTRACTOR_VERSION,
        Ok(Language::Elixir) => backhopper_elixir::EXTRACTOR_VERSION,
        Err(_) => backhopper_erlang::EXTRACTOR_VERSION,
    }
}

/// Emit one `tracing::warn` event per pin whose stored extractor version
/// is older than the running binary. Pure observability: no errors.
pub fn warn_on_stale_extractors(report: &[PinCoverageRow]) {
    for entry in report {
        if let PinCoverageStatus::StaleExtractor { stored, expected } = &entry.status {
            tracing::warn!(
                project = %entry.pin.project,
                tag = %entry.pin.tag,
                stored = %stored,
                expected = %expected,
                "snapshot was generated by an older extractor; consider `backhopper snapshots generate`"
            );
        }
    }
}
