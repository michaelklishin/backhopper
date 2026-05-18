//! Filesystem-backed snapshot store with `ReadOnly`/`Mutable` type-state.

use std::fs;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};

use crate::errors::StoreError;
use crate::model::names::{ProjectName, TagName};
use crate::model::snapshot::{Snapshot, state};
use crate::snapshot::{format, parser};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadOnly;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mutable;

#[derive(Debug)]
pub struct SnapshotStore<M> {
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

impl<M> SnapshotStore<M> {
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
        let path = self.snapshot_path(project, tag)?;
        if !path.exists() {
            return Err(StoreError::SnapshotNotFound {
                project: project.to_string(),
                tag: tag.to_string(),
            });
        }
        let text = fs::read_to_string(&path)?;
        let snapshot = parser::parse(&text).map_err(StoreError::Snapshot)?;
        Ok(snapshot)
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
        let path = self.snapshot_path(&snapshot.header.project, &snapshot.header.tag)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = temp_sibling(&path);
        let mut file = fs::File::create(&temp)?;
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
