// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Filesystem-backed snapshot store with `ReadOnly`/`Mutable` type-state.

use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};

use crate::errors::StoreError;
use crate::model::names::{ProjectName, TagName};
use crate::model::snapshot::{Snapshot, SnapshotHeader, state};
use crate::snapshot::{format, parser};

mod private {
    pub trait Sealed {}
}

/// Sealed so a foreign marker cannot stand in for `ReadOnly` or `Mutable`
/// and give itself a `write`.
pub trait StoreMode: private::Sealed {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadOnly;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mutable;

impl private::Sealed for ReadOnly {}
impl private::Sealed for Mutable {}
impl StoreMode for ReadOnly {}
impl StoreMode for Mutable {}

#[derive(Debug)]
pub struct SnapshotStore<M: StoreMode> {
    root: PathBuf,
    _mode: PhantomData<M>,
}

const SNAPSHOT_SUFFIX: &str = ".api.txt";

impl SnapshotStore<ReadOnly> {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.exists() {
            return Err(StoreError::RootMissing(root));
        }
        Ok(Self {
            root,
            _mode: PhantomData,
        })
    }
}

impl SnapshotStore<Mutable> {
    pub fn open_mut(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            _mode: PhantomData,
        })
    }
}

impl<M: StoreMode> SnapshotStore<M> {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn project_dir(&self, project: &ProjectName) -> PathBuf {
        self.root.join(project.as_str())
    }

    pub fn snapshot_path(
        &self,
        project: &ProjectName,
        tag: &TagName,
    ) -> Result<PathBuf, StoreError> {
        let mut path = self.project_dir(project);
        let filename = format!("{}{}", tag.as_str(), SNAPSHOT_SUFFIX);
        path.push(&filename);
        ensure_no_parent_escape(&path)?;
        Ok(path)
    }

    fn existing_snapshot_path(
        &self,
        project: &ProjectName,
        tag: &TagName,
    ) -> Result<PathBuf, StoreError> {
        let path = self.snapshot_path(project, tag)?;
        if path.exists() {
            Ok(path)
        } else {
            Err(StoreError::SnapshotNotFound {
                project: project.to_string(),
                tag: tag.to_string(),
            })
        }
    }

    pub fn list_tags(&self, project: &ProjectName) -> Result<Vec<TagName>, StoreError> {
        let dir = self.project_dir(project);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut tags = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(stripped) = name.strip_suffix(SNAPSHOT_SUFFIX) else {
                continue;
            };
            if let Ok(tag) = stripped.parse::<TagName>() {
                tags.push(tag);
            }
        }
        tags.sort();
        Ok(tags)
    }

    pub fn read(
        &self,
        project: &ProjectName,
        tag: &TagName,
    ) -> Result<Snapshot<state::Canonical>, StoreError> {
        let text = self.read_text(project, tag)?;
        let snapshot = parser::parse(&text).map_err(StoreError::Snapshot)?;
        Ok(snapshot)
    }

    /// The snapshot file as stored, so a caller can parse its header and
    /// its body from one read.
    pub fn read_text(&self, project: &ProjectName, tag: &TagName) -> Result<String, StoreError> {
        let path = self.existing_snapshot_path(project, tag)?;
        Ok(fs::read_to_string(&path)?)
    }

    /// Reads and parses only the header block of a snapshot file.
    pub fn read_header(
        &self,
        project: &ProjectName,
        tag: &TagName,
    ) -> Result<SnapshotHeader, StoreError> {
        let path = self.existing_snapshot_path(project, tag)?;
        let mut header_text = String::new();
        for line in BufReader::new(fs::File::open(&path)?).lines() {
            let line = line?;
            if !line.starts_with("# ") {
                break;
            }
            header_text.push_str(&line);
            header_text.push('\n');
        }
        parser::parse_header(&header_text).map_err(StoreError::Snapshot)
    }

    pub fn has(&self, project: &ProjectName, tag: &TagName) -> bool {
        self.snapshot_path(project, tag)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectName>, StoreError> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Ok(p) = name.parse::<ProjectName>() {
                out.push(p);
            }
        }
        out.sort();
        Ok(out)
    }
}

impl SnapshotStore<Mutable> {
    pub fn write(&self, snapshot: &Snapshot<state::Canonical>) -> Result<PathBuf, StoreError> {
        let path = self.snapshot_path(&snapshot.header().project, &snapshot.header().tag)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = temp_sibling(&path);
        let mut file = BufWriter::new(fs::File::create(&temp)?);
        format::write(snapshot, &mut file).map_err(StoreError::Snapshot)?;
        file.flush()?;
        drop(file);
        fs::rename(&temp, &path)?;
        Ok(path)
    }

    pub fn delete(&self, project: &ProjectName, tag: &TagName) -> Result<(), StoreError> {
        let path = self.snapshot_path(project, tag)?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }
}

fn ensure_no_parent_escape(candidate: &Path) -> Result<(), StoreError> {
    for c in candidate.components() {
        if matches!(c, Component::ParentDir) {
            return Err(StoreError::PathEscape(candidate.to_path_buf()));
        }
    }
    Ok(())
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}
