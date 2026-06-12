// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Parse RabbitMQ's `rabbitmq-components.mk` dep pins.
//!
//! Three real-world line shapes:
//!  * `dep_NAME = hex VERSION`           (e.g. `dep_ra = hex 3.1.6`)
//!  * `dep_NAME = git URL TAG`           (e.g. `dep_osiris = git https://... v1.13.1`)
//!  * `dep_NAME = git_rmq NAME TAG`      (vendored fork: `dep_cowboy = git_rmq cowboy 2.13.0`)
//!
//! `hex` versions are bare (`3.1.6`); `git` and `git_rmq` versions are real
//! tag names. Callers map a `hex` version back to a tag by applying the
//! project's `tag_prefix` from config.
//!
//! This module is RabbitMQ-specific glue and lives in the CLI: the
//! `backhopper-core` library has no business knowing about a particular
//! project's makefile conventions.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use backhopper_core::compat::patch::{HunkLine, PatchedFile};
use backhopper_core::model::names::DependencyName;
use backhopper_core::model::verdict::PinBump;
use regex::Regex;

pub use backhopper_git::COMPONENTS_MK_PATH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepPin {
    pub name: String,
    pub version: String,
    pub source: DepSource,
}

impl DepPin {
    /// The envelope display form, e.g. `hex 2.17.1`.
    pub fn display(&self) -> String {
        format!("{} {}", self.source.label(), self.version)
    }

    /// Inverse of `display`, for reading the form back out of an envelope.
    pub fn parse_display(name: &str, s: &str) -> Option<Self> {
        let (label, version) = s.split_once(' ')?;
        let source = match label {
            "hex" => DepSource::Hex,
            "git" => DepSource::Git,
            "git_rmq" => DepSource::GitRmq,
            _ => return None,
        };
        Some(Self {
            name: name.to_owned(),
            version: version.to_owned(),
            source,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSource {
    Hex,
    Git,
    GitRmq,
}

impl DepSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Git => "git",
            Self::GitRmq => "git_rmq",
        }
    }
}

fn dep_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\s*dep_([a-z0-9_]+)\s*=\s*(hex|git_rmq|git)\s+(.+?)\s*$").expect("regex")
    })
}

/// Parse a single makefile line into a dep pin.
pub fn parse_pin_line(line: &str) -> Option<DepPin> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let caps = dep_re().captures(line)?;
    let name = caps[1].to_owned();
    if name.ends_with("_commit") || name.ends_with("_branch") || name.ends_with("_repo") {
        return None;
    }
    let source_label = &caps[2];
    let rest = caps[3].trim();
    let (source, version) = match source_label {
        "hex" => (
            DepSource::Hex,
            rest.split_whitespace().next().unwrap_or("").to_owned(),
        ),
        "git" => (DepSource::Git, parse_git_url_tag(rest)?),
        "git_rmq" => (DepSource::GitRmq, parse_git_rmq(rest)?),
        _ => return None,
    };
    if version.is_empty() {
        return None;
    }
    Some(DepPin {
        name,
        version,
        source,
    })
}

pub fn parse_components_mk(text: &str) -> Vec<DepPin> {
    text.lines().filter_map(parse_pin_line).collect()
}

/// Pins the patch itself changes or introduces in the root
/// `rabbitmq-components.mk`. Pure function of the patch: cacheable;
/// `status` stays `None` here and is assessed against the store later.
pub fn detect_pin_bumps(files: &[PatchedFile]) -> Vec<PinBump> {
    let mut removed: BTreeMap<String, DepPin> = BTreeMap::new();
    let mut added: BTreeMap<String, DepPin> = BTreeMap::new();
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
        let Ok(dep) = DependencyName::new(name) else {
            continue;
        };
        bumps.push(PinBump {
            dep,
            from: from.map(DepPin::display),
            to: to.display(),
            status: None,
        });
    }
    bumps
}

fn parse_git_url_tag(rest: &str) -> Option<String> {
    let mut parts = rest.split_whitespace();
    let _url = parts.next()?;
    let tag = parts.next()?;
    Some(tag.to_owned())
}

fn parse_git_rmq(rest: &str) -> Option<String> {
    let mut parts = rest.split_whitespace();
    let _name = parts.next()?;
    let tag = parts.next()?;
    Some(tag.to_owned())
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

pub fn version_to_tag(version: &str, tag_prefix: &str) -> String {
    if version.starts_with(tag_prefix) {
        version.to_owned()
    } else {
        format!("{tag_prefix}{version}")
    }
}

/// Hex deps get the project's tag prefix; git deps use the version verbatim.
pub fn dep_to_tag(pin: &DepPin, tag_prefix: &str) -> String {
    match pin.source {
        DepSource::Hex => version_to_tag(&pin.version, tag_prefix),
        DepSource::Git | DepSource::GitRmq => pin.version.clone(),
    }
}
