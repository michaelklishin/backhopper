// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! RabbitMQ's branch and backport conventions around `rabbitmq-components.mk`.
//!
//! The pin line parser is generic erlang.mk and lives in
//! `backhopper_core::erlang_mk`. What stays here is about RabbitMQ's branch
//! names and the backport workflow.

use std::collections::BTreeMap;
use std::path::Path;

use backhopper_core::compat::patch::{HunkLine, PatchedFile};
use backhopper_core::erlang_mk::{DepPin, parse_pin_line};
pub use backhopper_core::erlang_mk::{dep_to_tag, version_to_tag};
use backhopper_core::model::names::DependencyName;
use backhopper_core::model::verdict::PinBump;

pub use backhopper_git::COMPONENTS_MK_PATH;

/// Pins the patch itself changes or introduces in the root
/// `rabbitmq-components.mk`. Pure function of the patch: cacheable;
/// `status` stays `None` here and is assessed against the store later.
pub fn detect_pin_bumps(files: &[PatchedFile]) -> Vec<PinBump> {
    let mut removed: BTreeMap<DependencyName, DepPin> = BTreeMap::new();
    let mut added: BTreeMap<DependencyName, DepPin> = BTreeMap::new();
    for file in files {
        let path = file.new_path.as_deref().or(file.old_path.as_deref());
        if path != Some(Path::new(COMPONENTS_MK_PATH)) {
            continue;
        }
        for hunk in &file.hunks {
            for line in &hunk.lines {
                match line {
                    HunkLine::Added(text) => {
                        if let Some(pin) = parse_pin_line(text) {
                            added.insert(pin.name.clone(), pin);
                        }
                    }
                    HunkLine::Removed(text) => {
                        if let Some(pin) = parse_pin_line(text) {
                            removed.insert(pin.name.clone(), pin);
                        }
                    }
                    HunkLine::Context(_) => {}
                }
            }
        }
    }
    let mut bumps = Vec::new();
    // removed-only pins need no vetting; added and changed ones do
    for (name, to) in added {
        let from = removed.get(&name);
        if from.is_some_and(|f| f.version == to.version && f.source == to.source) {
            continue;
        }
        bumps.push(PinBump {
            dep: name,
            from: from.map(DepPin::display),
            to: to.display(),
            status: None,
        });
    }
    bumps
}

pub fn series_name_for_branch(branch: &str) -> String {
    let trimmed = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let trimmed = trimmed.strip_prefix("refs/tags/").unwrap_or(trimmed);
    if trimmed == "main" || trimmed == "master" {
        return format!("rabbitmq-{trimmed}");
    }
    let stripped = trimmed.strip_suffix(".x").unwrap_or(trimmed);
    let stripped = stripped.strip_prefix('v').unwrap_or(stripped);
    format!("rabbitmq-{stripped}")
}
