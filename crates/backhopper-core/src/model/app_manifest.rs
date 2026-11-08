// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! What an app declares it needs at run time, as one snapshot records it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::names::{ApplicationName, MakeVariableName, vocabulary};

vocabulary! {
    /// The file an app's manifest is read from.
    #[derive(Default)]
    pub enum ManifestSource : "manifest source" {
        #[default]
        AppSrc => "app_src",
        ErlangMk => "erlang_mk",
    }
}

/// Why a manifest's requirements could not be listed. A partial list
/// would make an app look shippable when it is not, or the reverse, so
/// none is kept.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ManifestUnreadable {
    Missing,
    UnexpandedVariable(MakeVariableName),
    ParseFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestDeps {
    Listed(BTreeSet<ApplicationName>),
    Unreadable(ManifestUnreadable),
}

/// `requires` is the union of `applications` and `included_applications`
/// for `.app.src`, and of `DEPS` and `LOCAL_DEPS` for erlang.mk: what the
/// runtime needs present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppManifest {
    pub name: ApplicationName,
    pub source: ManifestSource,
    pub requires: ManifestDeps,
}

/// An app manifest whose `requires` could be read. The only way to make
/// one is `AppManifest::into_read`, so nothing that holds one matches on
/// an unreadable list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadManifest {
    name: ApplicationName,
    source: ManifestSource,
    requires: BTreeSet<ApplicationName>,
}

impl ReadManifest {
    #[must_use]
    pub fn name(&self) -> &ApplicationName {
        &self.name
    }

    #[must_use]
    pub fn source(&self) -> ManifestSource {
        self.source
    }

    #[must_use]
    pub fn requires(&self) -> &BTreeSet<ApplicationName> {
        &self.requires
    }
}

impl AppManifest {
    pub fn into_read(self) -> Result<ReadManifest, ManifestUnreadable> {
        match self.requires {
            ManifestDeps::Listed(requires) => Ok(ReadManifest {
                name: self.name,
                source: self.source,
                requires,
            }),
            ManifestDeps::Unreadable(reason) => Err(reason),
        }
    }
}
