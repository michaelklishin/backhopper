// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The configuration keys and validators a schema or partial file defines.

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::model::names::{ConfKey, ValidatorName};

use crate::fragments::{CuttlefishFragment, FragmentKind};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaElements {
    pub mappings: BTreeSet<ConfKey>,
    pub validators: BTreeSet<ValidatorName>,
}

impl SchemaElements {
    #[must_use]
    pub fn defined_by(fragments: &[CuttlefishFragment]) -> Self {
        let mut elements = Self::default();
        for fragment in fragments {
            let Some(key) = fragment.key.as_deref() else {
                continue;
            };
            match fragment.kind {
                FragmentKind::Mapping => {
                    elements.mappings.extend(ConfKey::from_str(key).ok());
                }
                FragmentKind::Validator => {
                    elements
                        .validators
                        .extend(ValidatorName::from_str(key).ok());
                }
                FragmentKind::Translation | FragmentKind::IncludePartial => {}
            }
        }
        elements
    }
}
