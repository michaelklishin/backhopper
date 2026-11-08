// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Reads a plugin set from plain directories with `std::fs`, without git.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use backhopper_core::app_src::{self, SKIP_DIRS};
use backhopper_core::erlang_mk::ErlangMkManifest;
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginSources, SourceFile};
use backhopper_core::model::app_manifest::{ManifestUnreadable, ReadManifest};
use backhopper_core::model::names::{ApplicationName, RelativePath};

const SCHEMA_DIR: &str = "priv/schema";

#[derive(Debug, Error)]
pub enum PluginTreeError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{} is not a UTF-8 path", path.display())]
    NotUtf8 { path: PathBuf },
    #[error("{} holds no app: an app has a src/ directory and either src/<name>.app.src or a Makefile that sets PROJECT", dir.display())]
    NoApps { dir: PathBuf },
    #[error("the manifest of app {app} in {} cannot be read ({reason:?}): its requirements decide which apps it needs, so they are not guessed", root.display())]
    UnreadableManifest {
        app: ApplicationName,
        root: PathBuf,
        reason: ManifestUnreadable,
    },
    #[error("{}: {message}", path.display())]
    Manifest { path: PathBuf, message: String },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> PluginTreeError + '_ {
    |source| PluginTreeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The apps under `dirs`. An app root is a directory with a `src/` and a
/// manifest: `src/<name>.app.src`, or a `Makefile` that sets `PROJECT`,
/// with `.app.src` taken when both exist. Inside an app root only its
/// `deps/` is searched for more apps. A `deps/` app that `host_apps`
/// already provides is left out and listed as host-provided.
pub fn walk(
    dirs: &[PathBuf],
    host_apps: &BTreeSet<ApplicationName>,
) -> Result<PluginSources, PluginTreeError> {
    let mut sources = PluginSources::default();
    for dir in dirs {
        let before = sources.apps.len();
        search(dir, host_apps, &mut sources)?;
        if sources.apps.len() == before {
            return Err(PluginTreeError::NoApps { dir: dir.clone() });
        }
    }
    Ok(sources)
}

fn search(
    dir: &Path,
    host_apps: &BTreeSet<ApplicationName>,
    sources: &mut PluginSources,
) -> Result<(), PluginTreeError> {
    if let Some(app) = read_app(dir)? {
        sources.apps.push(app);
        return search_deps(dir, host_apps, sources);
    }
    for child in subdirectories(dir)? {
        search(&child, host_apps, sources)?;
    }
    Ok(())
}

/// Apps fetched into an app's `deps/`: the host's own are not part of the
/// plugin set.
fn search_deps(
    root: &Path,
    host_apps: &BTreeSet<ApplicationName>,
    sources: &mut PluginSources,
) -> Result<(), PluginTreeError> {
    let deps = root.join("deps");
    if !deps.is_dir() {
        return Ok(());
    }
    for child in subdirectories(&deps)? {
        let Some(app) = read_app(&child)? else {
            continue;
        };
        let name = app.manifest.name().clone();
        if host_apps.contains(&name) {
            sources.host_provided.insert(name);
            continue;
        }
        sources.apps.push(app);
        search_deps(&child, host_apps, sources)?;
    }
    Ok(())
}

fn subdirectories(dir: &Path) -> Result<Vec<PathBuf>, PluginTreeError> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let path = entry.map_err(io_error(dir))?.path();
        let skipped = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| SKIP_DIRS.contains(&name) || name.starts_with('.'));
        if path.is_dir() && !skipped {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn read_app(root: &Path) -> Result<Option<PluginAppSources>, PluginTreeError> {
    let src = root.join("src");
    if !src.is_dir() {
        return Ok(None);
    }
    let Some(manifest) = read_manifest(root, &src)? else {
        return Ok(None);
    };
    let name = manifest.manifest.name().clone();
    let mut files = Vec::new();
    collect(root, &src, &["erl", "hrl"], true, &mut files)?;
    collect(root, &root.join("include"), &["hrl"], true, &mut files)?;
    collect(
        root,
        &root.join(SCHEMA_DIR),
        &["schema", "partial"],
        false,
        &mut files,
    )?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    tracing::debug!(app = %name, root = %root.display(), files = files.len(), "read plugin app");
    Ok(Some(PluginAppSources {
        root: root.to_path_buf(),
        manifest: manifest.manifest,
        manifest_path: manifest.path,
        files,
    }))
}

struct FoundManifest {
    manifest: ReadManifest,
    path: RelativePath,
}

fn read_manifest(root: &Path, src: &Path) -> Result<Option<FoundManifest>, PluginTreeError> {
    let mut app_srcs: Vec<PathBuf> = fs::read_dir(src)
        .map_err(io_error(src))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".app.src"))
        .collect();
    app_srcs.sort();
    let (manifest, relative) = if let Some(app_src) = app_srcs.first() {
        let text = fs::read_to_string(app_src).map_err(io_error(app_src))?;
        let spec = app_src::parse(app_src, &text).map_err(|message| PluginTreeError::Manifest {
            path: app_src.clone(),
            message,
        })?;
        let name = spec.name.clone();
        (spec.into_manifest(name), relative_to(root, app_src)?)
    } else {
        let makefile = root.join("Makefile");
        if !makefile.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(&makefile).map_err(io_error(&makefile))?;
        let parsed = ErlangMkManifest::parse(&text);
        let Some(project) = parsed.project.clone() else {
            return Ok(None);
        };
        (parsed.into_manifest(project), relative_to(root, &makefile)?)
    };
    let app = manifest.name.clone();
    let manifest = manifest
        .into_read()
        .map_err(|reason| PluginTreeError::UnreadableManifest {
            app,
            root: root.to_path_buf(),
            reason,
        })?;
    Ok(Some(FoundManifest {
        manifest,
        path: relative,
    }))
}

fn relative_to(root: &Path, path: &Path) -> Result<RelativePath, PluginTreeError> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let text = relative.to_str().ok_or_else(|| PluginTreeError::NotUtf8 {
        path: path.to_path_buf(),
    })?;
    RelativePath::new(text.replace('\\', "/")).map_err(|e| PluginTreeError::Manifest {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn collect(
    root: &Path,
    dir: &Path,
    extensions: &[&str],
    recursive: bool,
    out: &mut Vec<SourceFile>,
) -> Result<(), PluginTreeError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let path = entry.map_err(io_error(dir))?.path();
        if path.is_dir() {
            let skipped = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_none_or(|name| SKIP_DIRS.contains(&name));
            if recursive && !skipped {
                collect(root, &path, extensions, recursive, out)?;
            }
            continue;
        }
        let wanted = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.contains(&ext));
        if wanted {
            out.push(SourceFile {
                path: relative_to(root, &path)?,
                bytes: fs::read(&path).map_err(io_error(&path))?,
            });
        }
    }
    Ok(())
}
