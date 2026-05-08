//! Git access through `gix`. The whole crate's git seam lives here.

use std::path::{Path, PathBuf};

use crate::errors::GitError;
use crate::model::names::{CommitSha, TagName};

#[derive(Debug)]
pub struct GitRepo {
    repo: gix::Repository,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    pub name: TagName,
    pub commit: CommitSha,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobAtPath {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl GitRepo {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, GitError> {
        let path = path.into();
        let repo = gix::open(&path)
            .or_else(|_| gix::discover(&path))
            .map_err(|e| GitError::OpenFailed(e.to_string()))?;
        Ok(Self { repo, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list_tags(&self) -> Result<Vec<TagName>, GitError> {
        let platform = self
            .repo
            .references()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let iter = platform.tags().map_err(|e| GitError::Gix(e.to_string()))?;
        let mut tags: Vec<TagName> = Vec::new();
        for r in iter {
            let r = r.map_err(|e| GitError::Gix(e.to_string()))?;
            let full = r.name().as_bstr().to_string();
            if let Some(short) = full.strip_prefix("refs/tags/")
                && let Ok(t) = short.parse::<TagName>()
            {
                tags.push(t);
            }
        }
        tags.sort_by(|a, b| version_cmp(a.as_str(), b.as_str()));
        Ok(tags)
    }

    pub fn resolve_rev(&self, spec: &str) -> Result<CommitSha, GitError> {
        let object = self
            .repo
            .rev_parse_single(spec)
            .map_err(|_| GitError::CommitNotFound(spec.to_owned()))?;
        let id = object.detach();
        CommitSha::new(id.to_string()).map_err(|e| GitError::Gix(e.to_string()))
    }

    pub fn resolve_tag(&self, tag: &TagName) -> Result<CommitSha, GitError> {
        let spec = format!("refs/tags/{}^{{commit}}", tag);
        let object = self
            .repo
            .rev_parse_single(spec.as_str())
            .map_err(|_| GitError::TagNotFound(tag.to_string()))?;
        let id = object.detach();
        CommitSha::new(id.to_string()).map_err(|e| GitError::Gix(e.to_string()))
    }

    pub fn read_paths_at_tag(
        &self,
        tag: &TagName,
        path_filter: impl Fn(&str) -> bool,
    ) -> Result<Vec<BlobAtPath>, GitError> {
        let commit_id = self.resolve_tag(tag)?;
        self.read_paths_at_commit(&commit_id, path_filter)
    }

    pub fn read_paths_at_commit(
        &self,
        commit: &CommitSha,
        path_filter: impl Fn(&str) -> bool,
    ) -> Result<Vec<BlobAtPath>, GitError> {
        let oid = gix::ObjectId::from_hex(commit.as_str().as_bytes())
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let object = self
            .repo
            .find_object(oid)
            .map_err(|_| GitError::CommitNotFound(commit.to_string()))?;
        let commit_obj = object
            .try_into_commit()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let tree = commit_obj
            .tree()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let mut out = Vec::new();
        let mut stack: Vec<(PathBuf, gix::Tree<'_>)> = Vec::new();
        stack.push((PathBuf::new(), tree));
        while let Some((prefix, current)) = stack.pop() {
            for entry in current.iter() {
                let entry = entry.map_err(|e| GitError::Gix(e.to_string()))?;
                let mut path = prefix.clone();
                path.push(entry.filename().to_string());
                let mode = entry.mode();
                if mode.is_tree() {
                    let sub = entry
                        .object()
                        .map_err(|e| GitError::Gix(e.to_string()))?
                        .try_into_tree()
                        .map_err(|e| GitError::Gix(e.to_string()))?;
                    stack.push((path, sub));
                } else if mode.is_blob() {
                    let path_str = path.to_string_lossy();
                    if !path_filter(&path_str) {
                        continue;
                    }
                    let blob = entry.object().map_err(|e| GitError::Gix(e.to_string()))?;
                    out.push(BlobAtPath {
                        path,
                        bytes: blob.data.clone(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    pub fn branches_containing(&self, commit: &CommitSha) -> Result<Vec<String>, GitError> {
        let oid = gix::ObjectId::from_hex(commit.as_str().as_bytes())
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let platform = self
            .repo
            .references()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let iter = platform
            .local_branches()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let mut branches: Vec<String> = Vec::new();
        for r in iter {
            let r = r.map_err(|e| GitError::Gix(e.to_string()))?;
            let head = r.id().detach();
            if head == oid {
                let full = r.name().as_bstr().to_string();
                if let Some(short) = full.strip_prefix("refs/heads/") {
                    branches.push(short.to_owned());
                }
            }
        }
        Ok(branches)
    }

    pub fn path_exists_at_tag(&self, tag: &TagName, path: &str) -> Result<bool, GitError> {
        let blobs = self.read_paths_at_tag(tag, |p| p == path)?;
        Ok(!blobs.is_empty())
    }

    pub fn parents(&self, commit: &CommitSha) -> Result<Vec<CommitSha>, GitError> {
        let oid = gix::ObjectId::from_hex(commit.as_str().as_bytes())
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let object = self
            .repo
            .find_object(oid)
            .map_err(|_| GitError::CommitNotFound(commit.to_string()))?;
        let c = object
            .try_into_commit()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let mut out = Vec::new();
        for id in c.parent_ids() {
            out.push(CommitSha::new(id.to_string()).map_err(|e| GitError::Gix(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn parent_commit(&self, commit: &CommitSha) -> Result<Option<CommitSha>, GitError> {
        Ok(self.parents(commit)?.into_iter().next())
    }

    pub fn diff_commits_unified(
        &self,
        from: &CommitSha,
        to: &CommitSha,
        path_filter: impl Fn(&str) -> bool,
    ) -> Result<String, GitError> {
        let from_blobs = self.read_paths_at_commit(from, &path_filter)?;
        let to_blobs = self.read_paths_at_commit(to, &path_filter)?;
        let mut out = String::new();
        let mut from_map: std::collections::BTreeMap<PathBuf, Vec<u8>> =
            from_blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
        let to_map: std::collections::BTreeMap<PathBuf, Vec<u8>> =
            to_blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
        for (path, new_bytes) in &to_map {
            let old_bytes = from_map.remove(path).unwrap_or_default();
            if old_bytes == *new_bytes {
                continue;
            }
            append_unified_diff(&mut out, path, &old_bytes, new_bytes);
        }
        for (path, old_bytes) in from_map {
            append_unified_diff(&mut out, &path, &old_bytes, &[]);
        }
        Ok(out)
    }
}

fn append_unified_diff(out: &mut String, path: &Path, old: &[u8], new: &[u8]) {
    let display = path.display();
    let old_text = std::str::from_utf8(old).unwrap_or("");
    let new_text = std::str::from_utf8(new).unwrap_or("");
    let input = imara_diff::InternedInput::new(old_text, new_text);
    let mut diff = imara_diff::Diff::compute(imara_diff::Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let printer = imara_diff::BasicLineDiffPrinter(&input.interner);
    let body = diff
        .unified_diff(&printer, imara_diff::UnifiedDiffConfig::default(), &input)
        .to_string();
    if body.is_empty() {
        return;
    }
    out.push_str(&format!("diff --git a/{} b/{}\n", display, display));
    out.push_str(&format!("--- a/{}\n", display));
    out.push_str(&format!("+++ b/{}\n", display));
    out.push_str(&body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts = parse_version(a);
    let b_parts = parse_version(b);
    a_parts.cmp(&b_parts).reverse()
}

fn parse_version(tag: &str) -> Vec<u64> {
    let stripped = tag.strip_prefix('v').unwrap_or(tag);
    stripped
        .split(['.', '-', '+', '_'])
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}
