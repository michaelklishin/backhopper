// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The Cuttlefish files an app ships under `priv/schema/`.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::names::{ApplicationName, ConfKey, PartialName, RelativePath, ValidatorName};

/// A `*.schema` file: the keys it maps and the validators it defines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaFile {
    pub path: RelativePath,
    pub app: ApplicationName,
    pub mappings: BTreeSet<ConfKey>,
    pub validators: BTreeSet<ValidatorName>,
}

/// A `*.partial` file. Its mappings are bare keys, before the including
/// schema's prefix is applied: the names `exclude` and `overrides` use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialFile {
    pub path: RelativePath,
    pub app: ApplicationName,
    pub name: PartialName,
    pub mappings: BTreeSet<ConfKey>,
    pub validators: BTreeSet<ValidatorName>,
}

/// How `{include_partial, {App, Name}, ...}` addresses a partial.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartialRef {
    pub app: ApplicationName,
    pub name: PartialName,
}

impl PartialFile {
    #[must_use]
    pub fn partial_ref(&self) -> PartialRef {
        PartialRef {
            app: self.app.clone(),
            name: self.name.clone(),
        }
    }
}

impl fmt::Display for PartialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}, \"{}\"}}", self.app, self.name)
    }
}
