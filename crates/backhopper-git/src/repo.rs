// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `GitRepo`: the single wrapper around `gix::Repository`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::fmt::Write;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::str;

use gix::bstr::ByteSlice;
use gix::object::tree::diff::Change;
use imara_diff::{Algorithm, Diff, Hunk, InternedInput};
use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, CommitShaPrefix, TagName};
use backhopper_core::versions::version_cmp;

use crate::errors::{GitError, gix_err};
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
        self.peel_to_commit(prefix, oid_of(&full)?)
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
                let commit = CommitSha::new(id.to_string()).map_err(gix_err)?;
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
                let commit = CommitSha::new(commit_id.to_string()).map_err(gix_err)?;
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
        // indexed prefix lookup over the pack and loose object indices,
        // not a full object-store scan: the candidate set is every oid
        // sharing the prefix, capped to a sample for the error message
        let mut candidates: HashSet<gix::ObjectId> = HashSet::new();
        if self
            .repo
            .objects
            .lookup_prefix(hex_prefix, Some(&mut candidates))
            .is_err()
        {
            return PrefixLookup::None;
        }
        let total = candidates.len();
        if total <= 1 {
            PrefixLookup::None
        } else {
            let mut sample: Vec<gix::ObjectId> = candidates.into_iter().collect();
            sample.sort();
            sample.truncate(AMBIGUOUS_SHA_CANDIDATE_CAP);
            PrefixLookup::Ambiguous { total, sample }
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
        let platform = self.repo.references().map_err(gix_err)?;
        let iter = platform.tags().map_err(gix_err)?;
        let mut tags: Vec<TagName> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for r in iter {
            let r = r.map_err(gix_err)?;
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
        CommitSha::new(id.to_string()).map_err(gix_err)
    }

    pub fn resolve_tag(&self, tag: &TagName) -> Result<CommitSha, GitError> {
        let spec = format!("refs/tags/{tag}^{{commit}}");
        let object = self
            .repo
            .rev_parse_single(spec.as_str())
            .map_err(|_| GitError::TagNotFound(tag.to_string()))?;
        let id = object.detach();
        CommitSha::new(id.to_string()).map_err(gix_err)
    }

    pub fn read_paths_at_commit(
        &self,
        commit: &CommitSha,
        path_filter: impl Fn(&str) -> bool,
    ) -> Result<Vec<BlobAtPath>, GitError> {
        let mut out = Vec::new();
        self.walk_tree(commit, |path, entry| {
            if !path_filter(&path) {
                return Ok(());
            }
            let blob = entry.object().map_err(gix_err)?;
            out.push(BlobAtPath {
                path: PathBuf::from(path),
                bytes: blob.data.clone(),
            });
            Ok(())
        })?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Depth-first walk of `commit`'s tree, calling `on_blob` with each
    /// blob's path and tree entry. Tree names join with `/`, git's own
    /// separator, so the paths match diff-derived paths on every
    /// platform rather than picking up `\` from `PathBuf::push`.
    fn walk_tree(
        &self,
        commit: &CommitSha,
        mut on_blob: impl FnMut(String, &gix::object::tree::EntryRef<'_, '_>) -> Result<(), GitError>,
    ) -> Result<(), GitError> {
        let tree = self.tree_of(commit)?;
        let mut stack: Vec<(String, gix::Tree<'_>)> = vec![(String::new(), tree)];
        while let Some((prefix, current)) = stack.pop() {
            for entry in current.iter() {
                let entry = entry.map_err(gix_err)?;
                let name = entry.filename().to_string();
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                let mode = entry.mode();
                if mode.is_tree() {
                    let sub = entry
                        .object()
                        .map_err(gix_err)?
                        .try_into_tree()
                        .map_err(gix_err)?;
                    stack.push((path, sub));
                } else if mode.is_blob() {
                    on_blob(path, &entry)?;
                }
            }
        }
        Ok(())
    }

    /// Local branch names and the commit each tip points at.
    pub fn local_branch_tips(&self) -> Result<Vec<(String, CommitSha)>, GitError> {
        let platform = self.repo.references().map_err(gix_err)?;
        let iter = platform.local_branches().map_err(gix_err)?;
        let mut out: Vec<(String, CommitSha)> = Vec::new();
        for r in iter {
            let r = r.map_err(gix_err)?;
            let head = r.id().detach();
            let full = r.name().as_bstr().to_string();
            if let Some(short) = full.strip_prefix("refs/heads/") {
                let sha = CommitSha::new(head.to_string()).map_err(gix_err)?;
                out.push((short.to_owned(), sha));
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn branches_containing(&self, commit: &CommitSha) -> Result<Vec<String>, GitError> {
        let oid = oid_of(commit)?;
        let platform = self.repo.references().map_err(gix_err)?;
        let iter = platform.local_branches().map_err(gix_err)?;
        let mut branches: Vec<String> = Vec::new();
        for r in iter {
            let r = r.map_err(gix_err)?;
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
        let c = self.commit_of(commit)?;
        pr_commits::commit_subject(&c)
    }

    pub fn parents(&self, commit: &CommitSha) -> Result<Vec<CommitSha>, GitError> {
        let c = self.commit_of(commit)?;
        let mut out = Vec::new();
        for id in c.parent_ids() {
            out.push(CommitSha::new(id.to_string()).map_err(gix_err)?);
        }
        Ok(out)
    }

    /// Full raw commit message (subject plus body plus trailers).
    pub fn commit_message(&self, commit: &CommitSha) -> Result<String, GitError> {
        let c = self.commit_of(commit)?;
        Ok(c.message_raw()
            .map_err(gix_err)?
            .to_str_lossy()
            .into_owned())
    }

    /// True when the commit object exists in this repository's object
    /// store. An object of another kind under the same id does not
    /// count.
    pub fn has_commit(&self, commit: &CommitSha) -> Result<bool, GitError> {
        let oid = oid_of(commit)?;
        // header lookup reports the object kind without inflating the
        // full commit, so existence-plus-kind costs no object decode
        Ok(self
            .repo
            .find_header(oid)
            .is_ok_and(|header| header.kind() == gix::objs::Kind::Commit))
    }

    /// Best merge base of two commits, `None` when the histories are
    /// unrelated. Errors when either commit object is absent.
    pub fn merge_base(
        &self,
        one: &CommitSha,
        two: &CommitSha,
    ) -> Result<Option<CommitSha>, GitError> {
        for sha in [one, two] {
            if !self.has_commit(sha)? {
                return Err(GitError::CommitNotFound(sha.to_string()));
            }
        }
        let a = oid_of(one)?;
        let b = oid_of(two)?;
        match self.repo.merge_base(a, b) {
            Ok(id) => {
                let sha = CommitSha::new(id.to_string()).map_err(gix_err)?;
                Ok(Some(sha))
            }
            Err(gix::repository::merge_base::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(gix_err(e)),
        }
    }

    /// Paths changed between two commits: a tree diff that never reads
    /// blob contents, for cheap pre-filtering before a full text diff.
    pub fn changed_paths(
        &self,
        from: &CommitSha,
        to: &CommitSha,
    ) -> Result<BTreeSet<PathBuf>, GitError> {
        let from_tree = self.tree_of(from)?;
        let to_tree = self.tree_of(to)?;
        let mut out: BTreeSet<PathBuf> = BTreeSet::new();
        let mut platform = from_tree.changes().map_err(gix_err)?;
        platform
            .options(|opts| {
                opts.track_rewrites(None);
            })
            .for_each_to_obtain_tree(&to_tree, |change| -> Result<_, GitError> {
                match change {
                    Change::Addition {
                        location,
                        entry_mode,
                        ..
                    }
                    | Change::Deletion {
                        location,
                        entry_mode,
                        ..
                    }
                    | Change::Modification {
                        location,
                        entry_mode,
                        ..
                    } if entry_mode.is_blob() => {
                        out.insert(PathBuf::from(location.to_string()));
                    }
                    _ => {}
                }
                Ok(ControlFlow::Continue(()))
            })
            .map_err(gix_err)?;
        Ok(out)
    }

    pub fn parent_commit(&self, commit: &CommitSha) -> Result<Option<CommitSha>, GitError> {
        Ok(self.parents(commit)?.into_iter().next())
    }

    /// Committer timestamp of a commit: when it landed on its branch.
    pub fn commit_timestamp(&self, commit: &CommitSha) -> Result<OffsetDateTime, GitError> {
        let c = self.commit_of(commit)?;
        let time = c.time().map_err(gix_err)?;
        OffsetDateTime::from_unix_timestamp(time.seconds)
            .map_err(|e| GitError::Gix(format!("committer time out of range: {e}")))
    }

    /// True when `ancestor` is reachable from `descendant` (a commit
    /// counts as its own ancestor).
    pub fn is_ancestor(
        &self,
        ancestor: &CommitSha,
        descendant: &CommitSha,
    ) -> Result<bool, GitError> {
        let target = oid_of(ancestor)?;
        let from = oid_of(descendant)?;
        for info in self.repo.rev_walk([from]).all().map_err(gix_err)? {
            let info = info.map_err(gix_err)?;
            if info.id == target {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The subset of `candidates` reachable from `tip`. One history
    /// walk regardless of the candidate count; use this instead of
    /// repeated `is_ancestor` calls when screening many commits
    /// (e.g. every release tag of a project).
    pub fn ancestors_among(
        &self,
        tip: &CommitSha,
        candidates: &BTreeSet<CommitSha>,
    ) -> Result<BTreeSet<CommitSha>, GitError> {
        if candidates.is_empty() {
            return Ok(BTreeSet::new());
        }
        let from = oid_of(tip)?;
        let mut wanted: BTreeMap<gix::ObjectId, CommitSha> = BTreeMap::new();
        for sha in candidates {
            wanted.insert(oid_of(sha)?, sha.clone());
        }
        let mut found: BTreeSet<CommitSha> = BTreeSet::new();
        for info in self.repo.rev_walk([from]).all().map_err(gix_err)? {
            let info = info.map_err(gix_err)?;
            if let Some(sha) = wanted.get(&info.id) {
                found.insert(sha.clone());
                if found.len() == wanted.len() {
                    break;
                }
            }
        }
        Ok(found)
    }

    /// Unified diff between two commits, restricted to paths passing
    /// `path_filter`. Tree-diff based: only changed entries are
    /// visited and only their blobs are read, so the cost scales with
    /// the change, not with the tree.
    pub fn diff_commits_unified(
        &self,
        from: &CommitSha,
        to: &CommitSha,
        path_filter: impl Fn(&str) -> bool,
    ) -> Result<String, GitError> {
        let from_tree = self.tree_of(from)?;
        let to_tree = self.tree_of(to)?;
        // present on the `to` side (added or modified), then removed:
        // the same emission order as a full-tree map comparison
        let mut present: BTreeMap<PathBuf, (Vec<u8>, Vec<u8>)> = BTreeMap::new();
        let mut removed: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        let read_blob = |id: gix::Id<'_>| -> Result<Vec<u8>, GitError> {
            Ok(id.object().map_err(gix_err)?.data.clone())
        };
        let mut platform = from_tree.changes().map_err(gix_err)?;
        platform
            .options(|opts| {
                opts.track_rewrites(None);
            })
            .for_each_to_obtain_tree(&to_tree, |change| -> Result<_, GitError> {
                match change {
                    Change::Addition {
                        location,
                        entry_mode,
                        id,
                        ..
                    } if entry_mode.is_blob() => {
                        let path = PathBuf::from(location.to_string());
                        if path_filter(&path.to_string_lossy()) {
                            present.insert(path, (Vec::new(), read_blob(id)?));
                        }
                    }
                    Change::Deletion {
                        location,
                        entry_mode,
                        id,
                        ..
                    } if entry_mode.is_blob() => {
                        let path = PathBuf::from(location.to_string());
                        if path_filter(&path.to_string_lossy()) {
                            removed.insert(path, read_blob(id)?);
                        }
                    }
                    Change::Modification {
                        location,
                        previous_id,
                        id,
                        entry_mode,
                        ..
                    } if entry_mode.is_blob() && previous_id != id => {
                        let path = PathBuf::from(location.to_string());
                        if path_filter(&path.to_string_lossy()) {
                            present.insert(path, (read_blob(previous_id)?, read_blob(id)?));
                        }
                    }
                    _ => {}
                }
                Ok(ControlFlow::Continue(()))
            })
            .map_err(gix_err)?;
        let mut out = String::new();
        for (path, (old_bytes, new_bytes)) in &present {
            append_unified_diff(&mut out, path, old_bytes, new_bytes);
        }
        for (path, old_bytes) in &removed {
            append_unified_diff(&mut out, path, old_bytes, &[]);
        }
        Ok(out)
    }

    /// Every blob path at a commit, without reading blob contents.
    /// Use this instead of `read_paths_at_commit` when only the path
    /// set matters (e.g. building a `TargetTreeIndex`).
    pub fn list_paths_at_commit(&self, commit: &CommitSha) -> Result<Vec<PathBuf>, GitError> {
        let mut out: Vec<PathBuf> = Vec::new();
        self.walk_tree(commit, |path, _entry| {
            out.push(PathBuf::from(path));
            Ok(())
        })?;
        out.sort();
        Ok(out)
    }

    /// Read one blob at `path` inside `commit`'s tree. `Ok(None)`
    /// when the path is absent or names a non-blob entry.
    pub fn read_blob_at(
        &self,
        commit: &CommitSha,
        path: &Path,
    ) -> Result<Option<Vec<u8>>, GitError> {
        let tree = self.tree_of(commit)?;
        let Some(entry) = tree.lookup_entry_by_path(path).map_err(gix_err)? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        let object = entry.object().map_err(gix_err)?;
        Ok(Some(object.data.clone()))
    }

    fn tree_of(&self, commit: &CommitSha) -> Result<gix::Tree<'_>, GitError> {
        self.commit_of(commit)?.tree().map_err(gix_err)
    }

    /// Find `commit`'s object and peel it to a commit. A missing object
    /// maps to `CommitNotFound`; an object of another kind maps to
    /// `Gix`. The single place this crate turns a `CommitSha` into a
    /// `gix::Commit`.
    fn commit_of(&self, commit: &CommitSha) -> Result<gix::Commit<'_>, GitError> {
        self.repo
            .find_object(oid_of(commit)?)
            .map_err(|_| GitError::CommitNotFound(commit.to_string()))?
            .try_into_commit()
            .map_err(gix_err)
    }
}

/// Decode a `CommitSha` into a `gix::ObjectId`, mapping a malformed
/// hex string to `GitError::Gix`.
fn oid_of(commit: &CommitSha) -> Result<gix::ObjectId, GitError> {
    gix::ObjectId::from_hex(commit.as_str().as_bytes()).map_err(gix_err)
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
    // an empty side is an added or deleted file: git names it /dev/null,
    // and the core diff parser keys added-file detection on that token
    if old.is_empty() {
        let _ = writeln!(out, "--- /dev/null");
    } else {
        let _ = writeln!(out, "--- a/{display}");
    }
    if new.is_empty() {
        let _ = writeln!(out, "+++ /dev/null");
    } else {
        let _ = writeln!(out, "+++ b/{display}");
    }
    out.push_str(&body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
}

const DIFF_CONTEXT: u32 = 3;

/// Unified-diff hunks without any file header lines. Returns an empty
/// string when `before == after`. imara computes the change ranges;
/// the hunk grouping, headers, and line emission are ours: imara 0.2's
/// bundled printer can emit more leading context than its header start
/// accounts for, which shifts every file line a consumer derives from
/// `new_start`.
pub fn unified_diff_body(before: &str, after: &str) -> String {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<Hunk> = diff.hunks().collect();
    if changes.is_empty() {
        return String::new();
    }
    let before_len = input.before.len() as u32;
    // Group changes whose three-line contexts touch or overlap into one
    // printed hunk, the way git does.
    let mut groups: Vec<Vec<Hunk>> = Vec::new();
    for hunk in changes {
        match groups.last_mut() {
            Some(group)
                if hunk.before.start.saturating_sub(DIFF_CONTEXT)
                    <= group.last().expect("group is non-empty").before.end + DIFF_CONTEXT =>
            {
                group.push(hunk);
            }
            _ => groups.push(vec![hunk]),
        }
    }
    let before_line = |i: u32| input.interner[input.before[i as usize]];
    let after_line = |i: u32| input.interner[input.after[i as usize]];
    let mut out = String::new();
    for group in groups {
        let first = group.first().expect("group is non-empty");
        let last = group.last().expect("group is non-empty");
        // The unchanged region around a change is identical on both
        // sides, so one leading and one trailing count serve both.
        let leading = DIFF_CONTEXT.min(first.before.start);
        let trailing = DIFF_CONTEXT.min(before_len - last.before.end);
        let old_start = first.before.start - leading;
        let old_count = last.before.end + trailing - old_start;
        let new_start = first.after.start - leading;
        let new_count = last.after.end + trailing - new_start;
        let _ = writeln!(
            out,
            "@@ -{},{old_count} +{},{new_count} @@",
            display_start(old_start, old_count),
            display_start(new_start, new_count),
        );
        let mut pos = old_start;
        for hunk in &group {
            for i in pos..hunk.before.start {
                push_diff_line(&mut out, ' ', before_line(i));
            }
            for i in hunk.before.clone() {
                push_diff_line(&mut out, '-', before_line(i));
            }
            for i in hunk.after.clone() {
                push_diff_line(&mut out, '+', after_line(i));
            }
            pos = hunk.before.end;
        }
        for i in pos..last.before.end + trailing {
            push_diff_line(&mut out, ' ', before_line(i));
        }
    }
    out
}

/// Unified headers are 1-based, except that a zero-length range names
/// the line before the position.
fn display_start(start: u32, count: u32) -> u32 {
    if count == 0 { start } else { start + 1 }
}

fn push_diff_line(out: &mut String, sign: char, line: &str) {
    out.push(sign);
    out.push_str(line);
    if !line.ends_with('\n') {
        out.push('\n');
    }
}
