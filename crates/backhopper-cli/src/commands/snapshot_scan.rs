// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Reads the files a snapshot is built from at one commit, and says which
//! app each belongs to.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use backhopper_core::app_src::{self, AppSrcSpec};
use backhopper_core::config::{Language, Project, ProjectLayout};
use backhopper_core::erlang_mk::ErlangMkManifest;
use backhopper_core::extract::ScannedFile;
use backhopper_core::model::app_manifest::{
    AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable,
};
use backhopper_core::model::names::{ApplicationName, CommitSha, PartialName, RelativePath};
use backhopper_core::model::schema_files::{PartialFile, SchemaFile};
use backhopper_cuttlefish::{SchemaElements, parse_schema};
use backhopper_git::GitRepo;

use crate::errors::{CliError, CliResult};

const SCHEMA_DIR: &str = "priv/schema/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRole {
    Source,
    Manifest,
    Schema,
    Partial,
}

#[derive(Debug)]
pub(crate) struct ScannedTree {
    pub scanned_paths: Vec<String>,
    pub apps_scanned: Vec<ApplicationName>,
    pub sources: Vec<ScannedFile>,
    pub apps: Vec<AppManifest>,
    pub schemas: Vec<SchemaFile>,
    pub partials: Vec<PartialFile>,
}

struct RoleFile {
    role: FileRole,
    file: ScannedFile,
}

pub(crate) fn scan_tree(
    project: &Project,
    repo: &GitRepo,
    commit: &CommitSha,
) -> CliResult<ScannedTree> {
    match project.layout {
        ProjectLayout::SingleApp => scan_single_app(project, repo, commit),
        ProjectLayout::MultiApp | ProjectLayout::ErlangOtp => scan_multi_app(project, repo, commit),
    }
}

fn scan_multi_app(project: &Project, repo: &GitRepo, commit: &CommitSha) -> CliResult<ScannedTree> {
    let blobs =
        repo.read_paths_at_commit(commit, |path| classify_scan_path(path, project).is_some())?;
    let mut files = Vec::with_capacity(blobs.len());
    for blob in blobs {
        let path = relative_path(&blob.path)?;
        let Some((app, role, _)) = classify_scan_path(path.as_str(), project) else {
            continue;
        };
        files.push(RoleFile {
            role,
            file: ScannedFile {
                path,
                app,
                bytes: blob.bytes,
            },
        });
    }
    let apps: BTreeSet<ApplicationName> = files
        .iter()
        .filter(|f| f.role == FileRole::Source)
        .map(|f| f.file.app.clone())
        .collect();
    let apps_scanned: Vec<ApplicationName> = apps.iter().cloned().collect();
    assemble(
        project,
        project.app_roots.clone(),
        apps_scanned,
        &apps,
        files,
    )
}

fn scan_single_app(
    project: &Project,
    repo: &GitRepo,
    commit: &CommitSha,
) -> CliResult<ScannedTree> {
    let scan_paths = &project.scan_paths;
    let blobs = repo.read_paths_at_commit(commit, |path| {
        single_app_role(path, project).is_some() || matches_any(path, scan_paths)
    })?;
    let mut roled: Vec<(FileRole, RelativePath, Vec<u8>)> = Vec::with_capacity(blobs.len());
    for blob in blobs {
        let path = relative_path(&blob.path)?;
        let role = single_app_role(path.as_str(), project).unwrap_or(FileRole::Source);
        roled.push((role, path, blob.bytes));
    }
    let app = single_app_name(project, &roled)?;
    let files: Vec<RoleFile> = roled
        .into_iter()
        .map(|(role, path, bytes)| RoleFile {
            role,
            file: ScannedFile {
                path,
                app: app.clone(),
                bytes,
            },
        })
        .collect();
    let apps = BTreeSet::from([app]);
    assemble(project, scan_paths.clone(), Vec::new(), &apps, files)
}

/// The manifest's declared name when there is one, else the project name.
fn single_app_name(
    project: &Project,
    files: &[(FileRole, RelativePath, Vec<u8>)],
) -> CliResult<ApplicationName> {
    let declared = (project.language == Language::Erlang)
        .then(|| {
            files
                .iter()
                .filter(|(role, ..)| *role == FileRole::Manifest)
                .find_map(|(_, path, bytes)| {
                    ManifestFile::read(project.manifest, path, bytes)?.declared_name()
                })
        })
        .flatten();
    match declared {
        Some(name) => Ok(name),
        None => ApplicationName::from_str(project.name.as_str()).map_err(|e| {
            CliError::Other(format!(
                "project {} has no app manifest and its name is not an app name: {e}",
                project.name
            ))
        }),
    }
}

enum ManifestFile {
    AppSrc(AppSrcSpec),
    ErlangMk(ErlangMkManifest),
}

impl ManifestFile {
    fn read(source: ManifestSource, path: &RelativePath, bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        match source {
            ManifestSource::AppSrc => app_src::parse(Path::new(path.as_str()), text)
                .ok()
                .map(Self::AppSrc),
            ManifestSource::ErlangMk => Some(Self::ErlangMk(ErlangMkManifest::parse(text))),
        }
    }

    fn declared_name(&self) -> Option<ApplicationName> {
        match self {
            Self::AppSrc(spec) => Some(spec.name.clone()),
            Self::ErlangMk(manifest) => manifest.project.clone(),
        }
    }

    fn into_manifest(self, app: ApplicationName) -> AppManifest {
        match self {
            Self::AppSrc(spec) => spec.into_manifest(app),
            Self::ErlangMk(manifest) => manifest.into_manifest(app),
        }
    }
}

fn assemble(
    project: &Project,
    scanned_paths: Vec<String>,
    apps_scanned: Vec<ApplicationName>,
    apps: &BTreeSet<ApplicationName>,
    files: Vec<RoleFile>,
) -> CliResult<ScannedTree> {
    let erlang = project.language == Language::Erlang;
    let mut sources = Vec::new();
    let mut manifest_files: BTreeMap<ApplicationName, Vec<ScannedFile>> = BTreeMap::new();
    let mut schemas = Vec::new();
    let mut partials = Vec::new();
    for RoleFile { role, file } in files {
        match role {
            FileRole::Source => sources.push(file),
            FileRole::Manifest => manifest_files
                .entry(file.app.clone())
                .or_default()
                .push(file),
            FileRole::Schema if erlang => schemas.push(read_schema(file)?),
            FileRole::Partial if erlang => partials.push(read_partial(file)?),
            FileRole::Schema | FileRole::Partial => {}
        }
    }
    let manifests = if erlang {
        apps.iter()
            .map(|app| read_manifest(project.manifest, app, manifest_files.remove(app)))
            .collect()
    } else {
        Vec::new()
    };
    Ok(ScannedTree {
        scanned_paths,
        apps_scanned,
        sources,
        apps: manifests,
        schemas,
        partials,
    })
}

/// An app's manifest from the file candidates found for it: the one named
/// for the source, nearest the app root. No such file is `Missing`.
fn read_manifest(
    source: ManifestSource,
    app: &ApplicationName,
    candidates: Option<Vec<ScannedFile>>,
) -> AppManifest {
    let wanted = match source {
        ManifestSource::AppSrc => format!("{app}.app.src"),
        ManifestSource::ErlangMk => "Makefile".to_owned(),
    };
    let chosen = candidates
        .iter()
        .flatten()
        .filter(|c| c.path.as_str().rsplit('/').next() == Some(wanted.as_str()))
        .min_by_key(|c| c.path.as_str().matches('/').count());
    let unreadable = |reason| AppManifest {
        name: app.clone(),
        source,
        requires: ManifestDeps::Unreadable(reason),
    };
    let Some(file) = chosen else {
        return unreadable(ManifestUnreadable::Missing);
    };
    match ManifestFile::read(source, &file.path, &file.bytes) {
        Some(manifest) => manifest.into_manifest(app.clone()),
        None => unreadable(ManifestUnreadable::ParseFailed),
    }
}

fn read_schema(file: ScannedFile) -> CliResult<SchemaFile> {
    let elements = schema_elements(&file)?;
    Ok(SchemaFile {
        path: file.path,
        app: file.app,
        mappings: elements.mappings,
        validators: elements.validators,
    })
}

fn read_partial(file: ScannedFile) -> CliResult<PartialFile> {
    let elements = schema_elements(&file)?;
    let stem = Path::new(file.path.as_str())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let name =
        PartialName::from_str(stem).map_err(|e| CliError::Other(format!("{}: {e}", file.path)))?;
    Ok(PartialFile {
        path: file.path,
        app: file.app,
        name,
        mappings: elements.mappings,
        validators: elements.validators,
    })
}

fn schema_elements(file: &ScannedFile) -> CliResult<SchemaElements> {
    let text = std::str::from_utf8(&file.bytes)
        .map_err(|e| CliError::Other(format!("{}: invalid utf-8: {e}", file.path)))?;
    let fragments = parse_schema(text, Path::new(file.path.as_str()))
        .map_err(|e| CliError::Other(format!("{}: {e}", file.path)))?;
    Ok(SchemaElements::defined_by(&fragments))
}

fn relative_path(path: &Path) -> CliResult<RelativePath> {
    RelativePath::new(path.to_string_lossy().into_owned())
        .map_err(|e| CliError::Other(format!("{}: {e}", path.display())))
}

fn role_within(within: &str, manifest: ManifestSource) -> Option<FileRole> {
    if let Some(rest) = within.strip_prefix(SCHEMA_DIR)
        && !rest.contains('/')
    {
        if rest.ends_with(".schema") {
            return Some(FileRole::Schema);
        }
        if rest.ends_with(".partial") {
            return Some(FileRole::Partial);
        }
    }
    match manifest {
        ManifestSource::AppSrc => (within.starts_with("src/") && within.ends_with(".app.src"))
            .then_some(FileRole::Manifest),
        ManifestSource::ErlangMk => (within == "Makefile").then_some(FileRole::Manifest),
    }
}

fn single_app_role(path: &str, project: &Project) -> Option<FileRole> {
    role_within(path.trim_start_matches("./"), project.manifest)
}

/// Decide whether `path` belongs in a `multi_app` or `erlang_otp`
/// snapshot, which application owns it, what it is, and its path inside
/// the app. `None` drops the path.
pub(crate) fn classify_scan_path<'a>(
    path: &'a str,
    project: &Project,
) -> Option<(ApplicationName, FileRole, &'a str)> {
    let (app, within) = classify_app_path(path, &project.app_roots)?;
    if project.exclude_apps.iter().any(|a| a == &app) {
        return None;
    }
    if !project.include_apps.is_empty() && !project.include_apps.iter().any(|a| a == &app) {
        return None;
    }
    for sub in &project.excluded_subdirs {
        if let Some(rest) = within.strip_prefix(sub.as_str())
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return None;
        }
    }
    let role = if path.ends_with(".erl") || path.ends_with(".hrl") {
        FileRole::Source
    } else {
        role_within(within, project.manifest)?
    };
    Some((app, role, within))
}

/// Match `path` against `app_roots` and return `(app_name, path_within_app)`.
///
/// Two pattern shapes are supported:
///  * `<dir>/*`: a wildcard. The path must be `{root}/{app}/{within}`, where
///    the Erlang app name is `{app}`.
///  * `<dir>`: a literal directory. The path must be `{root}/{within}`, where
///    the Erlang app name is the last path segment of `{root}` (e.g.
///    `erts/preloaded` produces app `preloaded`).
fn classify_app_path<'a>(
    path: &'a str,
    app_roots: &[String],
) -> Option<(ApplicationName, &'a str)> {
    for pattern in app_roots {
        if let Some(stripped_root) = pattern.strip_suffix("/*") {
            if let Some(after_root) = path.strip_prefix(stripped_root)
                && let Some(after_sep) = after_root.strip_prefix('/')
                && let Some(slash) = after_sep.find('/')
                && let Ok(app) = ApplicationName::new(&after_sep[..slash])
            {
                return Some((app, &after_sep[slash + 1..]));
            }
        } else if let Some(after_root) = path.strip_prefix(pattern.as_str())
            && let Some(within) = after_root.strip_prefix('/')
        {
            let last = pattern.rsplit('/').next().unwrap_or(pattern);
            if let Ok(app) = ApplicationName::new(last) {
                return Some((app, within));
            }
        }
    }
    None
}

pub(crate) fn matches_any(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| glob_match(pat, path))
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let path = path.trim_start_matches("./");
    if let Some((dir, suffix)) = pattern.split_once("**/") {
        if path.starts_with(dir.trim_end_matches('/')) || dir.is_empty() {
            let rest = path
                .strip_prefix(dir.trim_end_matches('/'))
                .map(|s| s.trim_start_matches('/'))
                .unwrap_or(path);
            return suffix_match(suffix, rest);
        }
    }
    if let Some((dir, suffix)) = pattern.split_once("/*") {
        if path.starts_with(dir) {
            let rest = path.strip_prefix(dir).unwrap_or(path);
            let rest = rest.trim_start_matches('/');
            return suffix_match(suffix.trim_start_matches('/'), rest);
        }
    }
    pattern == path
}

fn suffix_match(suffix: &str, path: &str) -> bool {
    if let Some(ext) = suffix.strip_prefix("*.") {
        path.rsplit_once('.').is_some_and(|(_, e)| e == ext)
    } else {
        suffix.is_empty() || path.ends_with(suffix)
    }
}
