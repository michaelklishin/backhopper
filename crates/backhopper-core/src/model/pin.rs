// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::names::{ProjectName, TagName};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pin {
    pub project: ProjectName,
    pub tag: TagName,
}

impl Pin {
    pub fn new(project: ProjectName, tag: TagName) -> Self {
        Self { project, tag }
    }
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.project, self.tag)
    }
}
