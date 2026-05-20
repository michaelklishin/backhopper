// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Source locations.
//!
//! Locations are spans, not points, so multi-line call sites can be
//! reported verbatim. Paths are interned per [`CallGraph`](crate::graph::CallGraph)
//! to avoid storing thousands of duplicate `PathBuf`s on the edge set.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PathId(u32);

impl PathId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
    pub byte_offset: u32,
}

impl Position {
    pub const fn new(line: u32, column: u32, byte_offset: u32) -> Self {
        Self {
            line,
            column,
            byte_offset,
        }
    }

    /// `(1, 1, 0)`: the canonical "start of file" position.
    pub const fn zero() -> Self {
        Self::new(1, 1, 0)
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::zero()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Loc {
    pub path: PathId,
    pub start: Position,
    pub end: Position,
    pub expanded_from: Option<Box<Loc>>,
}

impl Loc {
    pub fn point(path: PathId, pos: Position) -> Self {
        Self {
            path,
            start: pos,
            end: pos,
            expanded_from: None,
        }
    }

    pub fn span(path: PathId, start: Position, end: Position) -> Self {
        Self {
            path,
            start,
            end,
            expanded_from: None,
        }
    }

    pub fn with_expansion(mut self, parent: Loc) -> Self {
        self.expanded_from = Some(Box::new(parent));
        self
    }
}

#[derive(Debug, Default, Clone)]
pub struct PathInterner {
    paths: Vec<PathBuf>,
    index: HashMap<PathBuf, PathId>,
}

impl PathInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, p: &Path) -> PathId {
        if let Some(id) = self.index.get(p) {
            return *id;
        }
        let id = PathId::new(u32::try_from(self.paths.len()).expect("path id overflow"));
        let owned = p.to_path_buf();
        self.paths.push(owned.clone());
        self.index.insert(owned, id);
        id
    }

    pub fn get(&self, id: PathId) -> Option<&Path> {
        self.paths.get(id.as_u32() as usize).map(PathBuf::as_path)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}
