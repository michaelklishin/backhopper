// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::Path;
use std::str::FromStr;

use backhopper_core::jigsaw::plugin_set::PluginFile;
use backhopper_core::model::names::{ConfKey, Mfa, PartialName, ValidatorName};
use backhopper_core::model::schema_files::PartialRef;
use backhopper_core::model::symbol::SymbolKind;
use backhopper_cuttlefish::{
    CuttlefishFragment, FragmentKind, ParseError, SchemaElements, extract_references, parse_schema,
};

/// One `priv/schema` file, parsed.
#[derive(Debug)]
pub(crate) struct SchemaFile {
    pub id: PluginFile,
    pub fragments: Vec<CuttlefishFragment>,
}

impl SchemaFile {
    pub(crate) fn parse(id: PluginFile, text: &str) -> Result<Self, ParseError> {
        let fragments = parse_schema(text, Path::new(id.path.as_str()))?;
        Ok(Self { id, fragments })
    }

    fn is_partial(&self) -> bool {
        self.id.path.as_str().ends_with(".partial")
    }

    /// The partial this file defines: its app and file stem.
    pub(crate) fn defined_partial(&self) -> Option<PartialRef> {
        if !self.is_partial() {
            return None;
        }
        let stem = Path::new(self.id.path.as_str()).file_stem()?.to_str()?;
        Some(PartialRef {
            app: self.id.app.clone(),
            name: PartialName::from_str(stem).ok()?,
        })
    }

    pub(crate) fn defined_validators(&self) -> BTreeSet<ValidatorName> {
        SchemaElements::defined_by(&self.fragments).validators
    }

    /// `include_partial` terms: the partial and the keys they exclude or
    /// override, with the line of the term.
    pub(crate) fn partial_includes(&self) -> Vec<(PartialRef, BTreeSet<ConfKey>, u32)> {
        self.fragments
            .iter()
            .filter_map(|fragment| {
                let include = fragment.partial_include.as_ref()?;
                let keys = include.exclude.union(&include.overrides).cloned().collect();
                Some((
                    PartialRef {
                        app: include.app.clone(),
                        name: include.name.clone(),
                    },
                    keys,
                    line(fragment.start_line),
                ))
            })
            .collect()
    }

    pub(crate) fn referenced_validators(&self) -> Vec<(ValidatorName, u32)> {
        self.fragments
            .iter()
            .filter(|fragment| fragment.kind == FragmentKind::Mapping)
            .flat_map(|fragment| {
                fragment
                    .referenced_validators
                    .iter()
                    .map(|name| (name.clone(), line(fragment.start_line)))
            })
            .collect()
    }

    /// Calls in translation, validator and partial-translation bodies.
    pub(crate) fn calls(&self) -> Vec<(Mfa, u32)> {
        self.fragments
            .iter()
            .flat_map(extract_references)
            .filter_map(|reference| match reference.symbol.kind {
                SymbolKind::Function { mfa } => Some((mfa, line(reference.line))),
                _ => None,
            })
            .collect()
    }
}

fn line(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
