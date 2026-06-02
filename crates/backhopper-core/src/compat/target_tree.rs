// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-touched-path classification against an optional target-branch
//! tree. Pure data; no I/O. CLI builds the inputs (`TargetTreeIndex`
//! plus `PathTranslations`) and merges the result into each pin's
//! verdict.

use std::path::{Path, PathBuf};

use crate::config::{PathTranslation, PathTranslations, TranslationOrigin};
use crate::git::TargetTreeIndex;
use crate::model::verdict::TranslationSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPathClassification {
    /// Path appears on the target tree as-is. No action.
    OnTarget,
    /// Path is absent on the target tree but a translation rewrites
    /// it to a path that does appear there.
    TranslatesTo {
        target_path: PathBuf,
        translation: TranslationSource,
    },
    /// Path is absent on the target tree and no translation matches.
    MissingFromTarget,
    /// Path was rejected at normalisation time (contained `..` or
    /// other unsafe shape). Skip without flagging.
    UnsafePath,
    /// Patch deletes this path; target-tree existence is irrelevant.
    DeletedByPatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedPathQuery<'a> {
    pub path: &'a Path,
    pub is_deletion: bool,
}

pub fn classify_path(
    query: TouchedPathQuery<'_>,
    index: &TargetTreeIndex,
    translations: &PathTranslations,
) -> TargetPathClassification {
    if query.is_deletion {
        return TargetPathClassification::DeletedByPatch;
    }
    let Some(normalised) = normalise(query.path) else {
        return TargetPathClassification::UnsafePath;
    };
    if index.contains_path(&normalised) {
        return TargetPathClassification::OnTarget;
    }
    let path_str = normalised.to_string_lossy();
    if let Some((rewritten, translation)) = translations.translate(&path_str) {
        if index.contains_path(&rewritten) {
            return TargetPathClassification::TranslatesTo {
                target_path: rewritten,
                translation: to_verdict_translation_source(translation),
            };
        }
    }
    TargetPathClassification::MissingFromTarget
}

fn to_verdict_translation_source(entry: &PathTranslation) -> TranslationSource {
    match &entry.origin {
        TranslationOrigin::ConfigStanza => TranslationSource::ConfigStanza {
            name: entry.name.clone(),
        },
        TranslationOrigin::ExternalFile { path } => TranslationSource::ExternalFile {
            path: path.clone(),
            name: entry.name.clone(),
        },
    }
}

/// Apply the path normalisation rules from doc 014 §3.5: backslash to
/// forward slash, strip leading `./`, reject `..` segments. Returns
/// `None` for unsafe paths.
pub fn normalise(p: &Path) -> Option<PathBuf> {
    let raw = p.to_string_lossy();
    let mut s: String = raw.replace('\\', "/");
    while s.starts_with("./") {
        s.drain(..2);
    }
    if s.split('/').any(|seg| seg == "..") {
        return None;
    }
    Some(PathBuf::from(s))
}
