// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The configuration a host snapshot's release roots and dep pins are read
//! with at generation.

use std::path::Path;

use backhopper_core::config::{Host, PinSource, Project};
use backhopper_core::erlang_mk::parse_components_mk;
use backhopper_core::jigsaw::host_facts_digest;
use backhopper_core::model::names::{CommitSha, HostFactsDigest, RelativePath, TagName};
use backhopper_core::model::release_roots::ReleaseRoots;
use backhopper_core::model::release_version::HostTag;
use backhopper_core::model::snapshot::{HostReleaseFacts, VendoredDep, VendoredDepSource};
use backhopper_git::GitRepo;

use crate::errors::{CliError, CliResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFacts {
    pub pins: PinSource,
    pub release_roots: ReleaseRoots,
    pub digest: HostFactsDigest,
}

impl HostFacts {
    #[must_use]
    pub fn for_host(host: &Host) -> Self {
        Self {
            pins: host.pins.clone(),
            release_roots: host.release_roots.clone(),
            digest: host_facts_digest(&host.pins, &host.release_roots),
        }
    }

    pub(crate) fn read_dep_pins(
        &self,
        project: &Project,
        repo: &GitRepo,
        commit: &CommitSha,
        tag: &TagName,
    ) -> CliResult<Vec<VendoredDep>> {
        match &self.pins {
            PinSource::ErlangMk { file_path } => {
                let text = read_text(repo, commit, file_path).ok_or_else(|| {
                    CliError::Other(format!(
                        "host pin file {file_path} is not readable at {commit}"
                    ))
                })?;
                Ok(parse_components_mk(&text))
            }
            PinSource::Table(table) => {
                let version = HostTag::parse(project, tag.clone())
                    .map_err(|e| CliError::Other(format!("host tag {tag}: {e}")))?
                    .version()
                    .clone();
                let deps = table.get(&version).ok_or_else(|| {
                    CliError::Other(format!("host pin table lists no release {version}"))
                })?;
                Ok(deps
                    .iter()
                    .map(|(name, version)| VendoredDep {
                        name: name.clone(),
                        version: version.clone(),
                        source: VendoredDepSource::Hex,
                    })
                    .collect())
            }
        }
    }

    pub(crate) fn read_release_facts(
        &self,
        repo: &GitRepo,
        commit: &CommitSha,
    ) -> CliResult<HostReleaseFacts> {
        let release_roots = self
            .release_roots
            .read(|path| read_text(repo, commit, path))
            .map_err(|e| CliError::Other(format!("release roots at {commit}: {e}")))?;
        Ok(HostReleaseFacts {
            release_roots,
            digest: self.digest.clone(),
        })
    }
}

fn read_text(repo: &GitRepo, commit: &CommitSha, path: &RelativePath) -> Option<String> {
    let bytes = repo
        .read_blob_at(commit, Path::new(path.as_str()))
        .ok()
        .flatten()?;
    String::from_utf8(bytes).ok()
}
