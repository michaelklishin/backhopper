// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fmt;

use crate::config::Project;
use crate::digest::DigestWriter;
use crate::model::names::{ExtractionSettingsDigest, ExtractorVersion};
use crate::model::snapshot::{FORMAT_VERSION, SnapshotHeader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleReason {
    Unreadable,
    FormatVersion { found: u32 },
    ExtractorVersion { found: String },
    ExtractionSettings,
    HostFacts,
    MissingApp,
}

impl fmt::Display for StaleReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("the file cannot be read"),
            Self::FormatVersion { found } => {
                write!(f, "format-version {found}, expected {FORMAT_VERSION}")
            }
            Self::ExtractorVersion { found } if found.is_empty() => {
                f.write_str("no extractor version recorded")
            }
            Self::ExtractorVersion { found } => write!(f, "extractor version {found}"),
            Self::ExtractionSettings => {
                f.write_str("made with other extraction settings than the configured ones")
            }
            Self::HostFacts => f.write_str("made with other host release facts"),
            Self::MissingApp => f.write_str("does not hold an app the plan needs"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedExtraction {
    pub extractor: ExtractorVersion,
    pub settings: ExtractionSettingsDigest,
}

// List settings are sets: their order in the config does not change a snapshot.
#[must_use]
pub fn extraction_settings_digest(project: &Project) -> ExtractionSettingsDigest {
    let mut writer = DigestWriter::new("extraction settings");
    writer
        .text(&project.layout.to_string())
        .text(&project.manifest.to_string());
    feed_set(&mut writer, project.scan_paths.iter().map(String::as_str));
    feed_set(&mut writer, project.app_roots.iter().map(String::as_str));
    feed_set(
        &mut writer,
        project.include_apps.iter().map(|app| app.as_str()),
    );
    feed_set(
        &mut writer,
        project.exclude_apps.iter().map(|app| app.as_str()),
    );
    feed_set(
        &mut writer,
        project.excluded_subdirs.iter().map(String::as_str),
    );
    feed_set(
        &mut writer,
        project.public_modules.iter().map(|module| module.as_str()),
    );
    feed_set(
        &mut writer,
        project
            .internal_modules
            .iter()
            .map(|module| module.as_str()),
    );
    ExtractionSettingsDigest::from_digest(writer.finish())
}

fn feed_set<'a>(writer: &mut DigestWriter, values: impl Iterator<Item = &'a str>) {
    let mut values: Vec<&str> = values.collect();
    values.sort_unstable();
    values.dedup();
    writer.count(values.len());
    for value in values {
        writer.text(value);
    }
}

#[must_use]
pub fn extraction_freshness(
    header: &SnapshotHeader,
    expected: &ExpectedExtraction,
) -> Option<StaleReason> {
    if header.extractor_version != expected.extractor.as_str() {
        return Some(StaleReason::ExtractorVersion {
            found: header.extractor_version.clone(),
        });
    }
    if header.format_version != FORMAT_VERSION {
        return Some(StaleReason::FormatVersion {
            found: header.format_version,
        });
    }
    if header.extraction_settings.as_ref() != Some(&expected.settings) {
        return Some(StaleReason::ExtractionSettings);
    }
    None
}
