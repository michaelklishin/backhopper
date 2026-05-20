// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Graph vertices.
//!
//! Vertices are entities the cross-reference engine tracks. We keep them as
//! a single enum over the existing `backhopper-core` newtypes so that a
//! `BTreeSet<Vertex>` can mix levels (function, module, application,
//! behaviour) when an analysis wants that.

use std::fmt;

use backhopper_core::{ApplicationName, BehaviourName, Mfa, ModuleName};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Vertex {
    Function(Mfa),
    Module(ModuleName),
    Application(ApplicationName),
    Behaviour(BehaviourName),
}

impl Vertex {
    pub fn as_function(&self) -> Option<&Mfa> {
        if let Vertex::Function(m) = self {
            Some(m)
        } else {
            None
        }
    }

    pub fn as_module(&self) -> Option<&ModuleName> {
        if let Vertex::Module(m) = self {
            Some(m)
        } else {
            None
        }
    }

    pub fn as_application(&self) -> Option<&ApplicationName> {
        if let Vertex::Application(a) = self {
            Some(a)
        } else {
            None
        }
    }

    pub fn as_behaviour(&self) -> Option<&BehaviourName> {
        if let Vertex::Behaviour(b) = self {
            Some(b)
        } else {
            None
        }
    }
}

impl fmt::Display for Vertex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Vertex::Function(mfa) => write!(f, "{}", mfa),
            Vertex::Module(m) => write!(f, "{}", m),
            Vertex::Application(a) => write!(f, "app:{}", a),
            Vertex::Behaviour(b) => write!(f, "behaviour:{}", b),
        }
    }
}
