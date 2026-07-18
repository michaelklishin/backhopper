// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Path prefix translations for cross-branch backport assessment.

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::PathTranslationRaw;
use crate::errors::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationDirection {
    SourceToTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathTranslation {
    pub name: String,
    pub direction: TranslationDirection,
    pub source_prefix: String,
    pub target_prefix: String,
    pub origin: TranslationOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranslationOrigin {
    ConfigStanza,
    ExternalFile { path: PathBuf },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathTranslations {
    pub entries: Vec<PathTranslation>,
}

impl PathTranslations {
    pub fn from_config_stanzas(raws: Vec<PathTranslationRaw>) -> Result<Self, ConfigError> {
        let entries = raws
            .into_iter()
            .map(|r| parse_one(r, TranslationOrigin::ConfigStanza))
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Self { entries };
        out.validate_uniqueness_and_overlap()?;
        out.sort_by_specificity();
        Ok(out)
    }

    pub fn load_external(path: &Path) -> Result<Vec<PathTranslation>, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::PathTranslationFileMissing(path.to_path_buf()));
        }
        let text = fs::read_to_string(path)?;
        let raw: ExternalFile = toml::from_str(&text)?;
        let origin = TranslationOrigin::ExternalFile {
            path: path.to_path_buf(),
        };
        raw.path_translations
            .into_iter()
            .map(|r| parse_one(r, origin.clone()))
            .collect()
    }

    pub fn merge_external(&mut self, external: Vec<PathTranslation>) -> Result<(), ConfigError> {
        self.entries.extend(external);
        self.validate_uniqueness_and_overlap()?;
        self.sort_by_specificity();
        Ok(())
    }

    // most specific (longest source prefix) first so translate can return on the first match
    fn sort_by_specificity(&mut self) {
        self.entries.sort_by_key(|e| Reverse(e.source_prefix.len()));
    }

    fn validate_uniqueness_and_overlap(&self) -> Result<(), ConfigError> {
        let mut seen_names: BTreeSet<&str> = BTreeSet::new();
        let mut seen_sources: BTreeSet<&str> = BTreeSet::new();
        for entry in &self.entries {
            if !seen_names.insert(&entry.name) {
                return Err(ConfigError::PathTranslationDuplicateName(
                    entry.name.clone(),
                ));
            }
            if !seen_sources.insert(&entry.source_prefix) {
                return Err(ConfigError::PathTranslationDuplicateSource(
                    entry.source_prefix.clone(),
                ));
            }
        }
        for (i, a) in self.entries.iter().enumerate() {
            for b in self.entries.iter().skip(i + 1) {
                if a.source_prefix == b.source_prefix {
                    continue;
                }
                if a.source_prefix.starts_with(&b.source_prefix)
                    || b.source_prefix.starts_with(&a.source_prefix)
                {
                    return Err(ConfigError::PathTranslationPrefixOverlap {
                        a: a.source_prefix.clone(),
                        b: b.source_prefix.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Apply the first matching translation to `path`. `entries` is kept
    /// sorted by descending `source_prefix` length so a more specific prefix wins.
    pub fn translate(&self, path: &str) -> Option<(PathBuf, &PathTranslation)> {
        for entry in &self.entries {
            if let Some(suffix) = path.strip_prefix(&entry.source_prefix) {
                let rewritten = format!("{}{}", entry.target_prefix, suffix);
                return Some((PathBuf::from(rewritten), entry));
            }
        }
        None
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalFile {
    #[serde(default, rename = "path_translation")]
    path_translations: Vec<PathTranslationRaw>,
}

fn parse_one(
    raw: PathTranslationRaw,
    origin: TranslationOrigin,
) -> Result<PathTranslation, ConfigError> {
    if raw.name.is_empty() {
        return Err(ConfigError::PathTranslationEmptyName);
    }
    if raw.source_prefix.is_empty() {
        return Err(ConfigError::PathTranslationEmptyPrefix {
            name: raw.name.clone(),
            field: "source_prefix",
        });
    }
    if raw.target_prefix.is_empty() {
        return Err(ConfigError::PathTranslationEmptyPrefix {
            name: raw.name.clone(),
            field: "target_prefix",
        });
    }
    if !raw.source_prefix.ends_with('/') {
        return Err(ConfigError::PathTranslationPrefixNoTrailingSlash {
            name: raw.name.clone(),
            field: "source_prefix",
            value: raw.source_prefix.clone(),
        });
    }
    if !raw.target_prefix.ends_with('/') {
        return Err(ConfigError::PathTranslationPrefixNoTrailingSlash {
            name: raw.name.clone(),
            field: "target_prefix",
            value: raw.target_prefix.clone(),
        });
    }
    let direction = match raw.direction.as_str() {
        "source_to_target" => TranslationDirection::SourceToTarget,
        other => {
            return Err(ConfigError::PathTranslationUnknownDirection {
                name: raw.name.clone(),
                direction: other.to_owned(),
            });
        }
    };
    Ok(PathTranslation {
        name: raw.name,
        direction,
        source_prefix: raw.source_prefix,
        target_prefix: raw.target_prefix,
        origin,
    })
}
