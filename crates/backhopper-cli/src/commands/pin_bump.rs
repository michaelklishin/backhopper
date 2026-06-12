// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Post-cache assessment of detected pin bumps.
//!
//! Detection (`detect_pin_bumps`) is a pure function of the patch and
//! rides in the cached evaluation with `status: None`. The snapshot
//! store's contents are not a cache-key input, so the assessment here
//! runs after every cache lookup, on hits and misses alike: generating
//! a snapshot between two checks is reflected without invalidation.

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::config::Config;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{BumpStatus, PinBump, SeriesEvaluation};
use backhopper_core::store::{ReadOnly, SnapshotStore};

use crate::cli::GlobalArgs;
use crate::commands::auto_generate::{generate_one_pin, snapshot_generate_command};
use crate::commands::context::open_store_mut;
use crate::commands::rabbitmq_components::{DepPin, dep_to_tag};

/// Where a detected bump's `to` side leads.
enum BumpTarget {
    /// Not a tracked project; nothing to vet.
    Untracked,
    /// Tracked, but the envelope strings did not resolve to a pin.
    Unresolved,
    Pin(Pin),
}

fn bump_target(cfg: &Config, bump: &PinBump) -> BumpTarget {
    let Ok(project_name) = ProjectName::from_str(bump.dep.as_str()) else {
        return BumpTarget::Untracked;
    };
    let Ok(project) = cfg.project(&project_name) else {
        return BumpTarget::Untracked;
    };
    let Some(pin) = DepPin::parse_display(bump.dep.as_str(), &bump.to) else {
        return BumpTarget::Unresolved;
    };
    match TagName::from_str(&dep_to_tag(&pin, &project.tag_prefix)) {
        Ok(tag) => BumpTarget::Pin(Pin::new(project_name, tag)),
        Err(_) => BumpTarget::Unresolved,
    }
}

#[derive(Debug)]
pub struct PinBumpAssessor<'a> {
    cfg: &'a Config,
    store: &'a SnapshotStore<ReadOnly>,
}

impl<'a> PinBumpAssessor<'a> {
    pub fn new(cfg: &'a Config, store: &'a SnapshotStore<ReadOnly>) -> Self {
        Self { cfg, store }
    }

    pub fn apply(&self, evaluation: &mut SeriesEvaluation) {
        for bump in &mut evaluation.diagnostics.pin_bumps {
            bump.status = self.assess(bump);
        }
    }

    /// `None` means "could not assess" and renders as unassessed:
    /// quieter than a wrong claim in either direction.
    fn assess(&self, bump: &PinBump) -> Option<BumpStatus> {
        match bump_target(self.cfg, bump) {
            BumpTarget::Untracked => Some(BumpStatus::Untracked),
            BumpTarget::Unresolved => None,
            BumpTarget::Pin(pin) => {
                if self.store.has(&pin.project, &pin.tag) {
                    Some(BumpStatus::SnapshotPresent)
                } else {
                    Some(BumpStatus::SnapshotMissing {
                        note: format!("run: {}", snapshot_generate_command(&pin.project, &pin.tag)),
                    })
                }
            }
        }
    }
}

/// With `--auto-generate`, writes the bumped-to snapshots this commit
/// proves the next round will need. Best-effort: a tag the dep repo
/// does not have must surface as `SnapshotMissing`, not as an error.
/// One per command invocation: `attempted` keeps a multi-row batch
/// from retrying (and re-warning about) the same failed pin per row.
#[derive(Debug)]
pub struct BumpSnapshotGenerator<'a> {
    args: &'a GlobalArgs,
    cfg: &'a Config,
    attempted: BTreeSet<Pin>,
}

impl<'a> BumpSnapshotGenerator<'a> {
    pub fn new(args: &'a GlobalArgs, cfg: &'a Config) -> Self {
        Self {
            args,
            cfg,
            attempted: BTreeSet::new(),
        }
    }

    pub fn apply(&mut self, store: &SnapshotStore<ReadOnly>, evaluation: &SeriesEvaluation) {
        let missing: Vec<Pin> = evaluation
            .diagnostics
            .pin_bumps
            .iter()
            .filter_map(|bump| match bump_target(self.cfg, bump) {
                BumpTarget::Pin(pin) => Some(pin),
                BumpTarget::Untracked | BumpTarget::Unresolved => None,
            })
            .filter(|pin| !store.has(&pin.project, &pin.tag))
            .filter(|pin| !self.attempted.contains(pin))
            .collect();
        if missing.is_empty() {
            return;
        }
        let mut_store = match open_store_mut(self.args, self.cfg) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "could not open the snapshot store for writing");
                return;
            }
        };
        for pin in missing {
            if let Err(e) = generate_one_pin(self.cfg, &mut_store, &pin) {
                tracing::warn!(
                    project = %pin.project,
                    tag = %pin.tag,
                    error = %e,
                    "could not generate the snapshot for a bumped pin"
                );
            }
            self.attempted.insert(pin);
        }
    }
}
