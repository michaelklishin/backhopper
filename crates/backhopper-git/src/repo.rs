// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `GitRepo`: the single wrapper around `gix::Repository`.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::str;

use imara_diff::{Algorithm, BasicLineDiffPrinter, Diff, InternedInput, UnifiedDiffConfig};

use backhopper_core::model::names::{CommitSha, CommitShaPrefix, TagName};
use backhopper_core::versions::version_cmp;

use crate::errors::GitError;
use crate::pr_commits;

pub const AMBIGUOUS_SHA_CANDIDATE_CAP: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSha {
    pub commit: CommitSha,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Commit,
    Tag,
    Blob,
    Tree,
}

enum PrefixLookup {
    None,
    Ambiguous {
        total: usize,
        sample: Vec<gix::ObjectId>,
    },
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ObjectKind::Commit => "commit",
            ObjectKind::Tag => "tag",
            ObjectKind::Blob => "blob",
            ObjectKind::Tree => "tree",
        })
    }
}

#[derive(Debug)]
pub struct GitRepo {
    repo: gix::Repository,
    path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagListing {
    pub tags: Vec<TagName>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobAtPath {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl GitRepo {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, GitError> {
        let path = path.into();
        if !path.exists() {
            return Err(GitError::NotAGitRepository(path));
        }
        let repo = gix::open(&path)
            .or_else(|_| gix::discover(&path))
            .map_err(|_| GitError::NotAGitRepository(path.clone()))?;
        Ok(Self { repo, path })
    }

    pub fn is_shallow(&self) -> bool {
        self.repo.is_shallow()
    }

    pub fn resolve_sha_prefix(&self, prefix: &CommitShaPrefix) -> Result<ResolvedSha, GitError> {
        if let Some(full) = prefix.as_full_sha() {
            return self.classify_full_sha(prefix, full);
        }
        match self.repo.rev_parse_single(prefix.as_str()) {
            Ok(object) => {
                let id = object.detach();
                self.peel_to_commit(prefix, id)
            }
            Err(_) => {
                let candidates = self.lookup_prefix_candidates(prefix);
                match candidates {
                    PrefixLookup::None => Err(GitError::CommitNotFound(prefix.to_string())),
                    PrefixLookup::Ambiguous { total, sample } => {
                        let truncated_at = total as u32;
                        let candidates: Vec<String> =
                            sample.iter().map(|o| o.to_string()).collect();
                        Err(GitError::AmbiguousSha {
                            prefix: prefix.to_string(),
                            candidates,
                            truncated_at,
                        })
                    }
                }
            }
        }
    }

    fn classify_full_sha(
        &self,
        prefix: &CommitShaPrefix,
        full: CommitSha,
    ) -> Result<ResolvedSha, GitError> {
        let oid = gix::ObjectId::from_hex(full.as_str().as_bytes())
            .map_err(|e| GitError::Gix(e.to_string()))?;
        self.peel_to_commit(prefix, oid)
    }

    fn peel_to_commit(
        &self,
        prefix: &CommitShaPrefix,
        id: gix::ObjectId,
    ) -> Result<ResolvedSha, GitError> {
        let object = self
            .repo
            .find_object(id)
            .map_err(|_| GitError::CommitNotFound(prefix.to_string()))?;
        let kind = match object.kind {
            gix::objs::Kind::Commit => ObjectKind::Commit,
            gix::objs::Kind::Tag => ObjectKind::Tag,
            gix::objs::Kind::Tree => ObjectKind::Tree,
            gix::objs::Kind::Blob => ObjectKind::Blob,
        };
        match kind {
            ObjectKind::Commit => {
                let commit =
                    CommitSha::new(id.to_string()).map_err(|e| GitError::Gix(e.to_string()))?;
                Ok(ResolvedSha {
                    commit,
                    kind: ObjectKind::Commit,
                })
            }
            ObjectKind::Tag => {
                let commit_object = object.peel_to_kind(gix::objs::Kind::Commit).map_err(|_| {
                    GitError::NotACommit {
                        prefix: prefix.to_string(),
                        kind: ObjectKind::Tag.to_string(),
                        resolved: id.to_string(),
                    }
                })?;
                let commit_id = commit_object.detach().id;
                let commit = CommitSha::new(commit_id.to_string())
                    .map_err(|e| GitError::Gix(e.to_string()))?;
                Ok(ResolvedSha {
                    commit,
                    kind: ObjectKind::Tag,
                })
            }
            other => Err(GitError::NotACommit {
                prefix: prefix.to_string(),
                kind: other.to_string(),
                resolved: id.to_string(),
            }),
        }
    }

    fn lookup_prefix_candidates(&self, prefix: &CommitShaPrefix) -> PrefixLookup {
        let Ok(hex_prefix) = gix::hash::Prefix::from_hex(prefix.as_str()) else {
            return PrefixLookup::None;
        };
        let mut candidates: Vec<gix::ObjectId> = Vec::new();
        let mut total: usize = 0;
        for oid in self
            .repo
            .objects
            .iter()
            .ok()
            .into_iter()
            .flatten()
            .flatten()
        {
            if hex_prefix.cmp_oid(&oid) == Ordering::Equal {
                total += 1;
                if candidates.len() < AMBIGUOUS_SHA_CANDIDATE_CAP {
                    candidates.push(oid);
                }
            }
        }
        if total <= 1 {
            PrefixLookup::None
        } else {
            candidates.sort();
            PrefixLookup::Ambiguous {
                total,
                sample: candidates,
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the underlying `gix::Repository`. Intended for sibling
    /// modules in this crate; not part of the stable public surface.
    pub(crate) fn gix(&self) -> &gix::Repository {
        &self.repo
    }

    pub fn list_tags(&self) -> Result<Vec<TagName>, GitError> {
        Ok(self.list_tag_refs()?.tags)
    }

    pub fn list_tag_refs(&self) -> Result<TagListing, GitError> {
        let platform = self
            .repo
            .references()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let iter = platform.tags().map_err(|e| GitError::Gix(e.to_string()))?;
        let mut tags: Vec<TagName> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for r in iter {
            let r = r.map_err(|e| GitError::Gix(e.to_string()))?;
            let full = r.name().as_bstr().to_string();
            let Some(short) = full.strip_prefix("refs/tags/") else {
                skipped.push(full);
                continue;
            };
            match short.parse::<TagName>() {
                Ok(t) => tags.push(t),
                Err(_) => skipped.push(short.to_owned()),
            }
        }
        tags.sort_by(|a, b| version_cmp(a.as_str(), b.as_str()));
        skipped.sort();
        Ok(TagListing { tags, skipped })
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
        let spec = format!("refs/tags/{tag}^{{commit}}");
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

    /// Return the one-line subject of a commit message.
    pub fn commit_subject(&self, commit: &CommitSha) -> Result<String, GitError> {
        let oid = gix::ObjectId::from_hex(commit.as_str().as_bytes())
            .map_err(|e| GitError::Gix(e.to_string()))?;
        let object = self
            .repo
            .find_object(oid)
            .map_err(|_| GitError::CommitNotFound(commit.to_string()))?;
        let c = object
            .try_into_commit()
            .map_err(|e| GitError::Gix(e.to_string()))?;
        pr_commits::commit_subject(&c)
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
        let mut from_map: BTreeMap<PathBuf, Vec<u8>> =
            from_blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
        let to_map: BTreeMap<PathBuf, Vec<u8>> =
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
    let old_text = str::from_utf8(old).unwrap_or("");
    let new_text = str::from_utf8(new).unwrap_or("");
    let body = unified_diff_body(old_text, new_text);
    if body.is_empty() {
        return;
    }
    let _ = writeln!(out, "diff --git a/{display} b/{display}");
    let _ = writeln!(out, "--- a/{display}");
    let _ = writeln!(out, "+++ b/{display}");
    out.push_str(&body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
}

/// Unified-diff hunks without any file header lines. Returns an empty
/// string when `before == after`.
pub fn unified_diff_body(before: &str, after: &str) -> String {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let printer = BasicLineDiffPrinter(&input.interner);
    diff.unified_diff(&printer, UnifiedDiffConfig::default(), &input)
        .to_string()
}
