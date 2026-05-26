// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared command context: load config, open store.

use std::env;
use std::path::{Component, Path, PathBuf};

use backhopper_core::Error as CoreError;
use backhopper_core::config::Config;
use backhopper_core::store::{Mutable, ReadOnly, SnapshotStore};

use crate::cli::GlobalArgs;
use crate::errors::{CliError, CliResult};

pub fn resolve_config_path(args: &GlobalArgs) -> CliResult<PathBuf> {
    if let Some(p) = &args.config_file_path {
        return Ok(p.clone());
    }
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir()
        && let Some(found) = walk_up_for_config(&cwd, &mut tried)
    {
        return Ok(found);
    }
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let candidate = PathBuf::from(xdg)
            .join("backhopper")
            .join("backhopper.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }
    if let Ok(home) = env::var("HOME") {
        let candidate = PathBuf::from(home)
            .join(".config")
            .join("backhopper")
            .join("backhopper.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }
    Err(CliError::ConfigNotFound { tried })
}

// Walk up from `start`, return the first `.backhopper.toml` or `backhopper.toml`
// found. Stops at a directory containing `.git`. Dotfile wins on a tie.
fn walk_up_for_config(start: &Path, tried: &mut Vec<PathBuf>) -> Option<PathBuf> {
    let mut here: Option<&Path> = Some(start);
    while let Some(dir) = here {
        for name in [".backhopper.toml", "backhopper.toml"] {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
            tried.push(candidate);
        }
        if dir.join(".git").exists() {
            break;
        }
        here = dir.parent();
    }
    None
}

pub fn load_config(args: &GlobalArgs) -> CliResult<Config> {
    let path = resolve_config_path(args)?;
    Config::load(&path).map_err(|e| CliError::Core(CoreError::Config(e)))
}

pub fn snapshot_dir(args: &GlobalArgs, cfg: &Config) -> PathBuf {
    if let Some(dir) = &args.snapshot_dir_path {
        return dir.clone();
    }
    cfg.snapshot_dir()
}

pub fn open_store_read(args: &GlobalArgs, cfg: &Config) -> CliResult<SnapshotStore<ReadOnly>> {
    let dir = snapshot_dir(args, cfg);
    check_snapshot_dir_escape(args, cfg, &dir)?;
    SnapshotStore::open(dir).map_err(|e| CliError::Core(CoreError::Store(e)))
}

pub fn open_store_mut(args: &GlobalArgs, cfg: &Config) -> CliResult<SnapshotStore<Mutable>> {
    let dir = snapshot_dir(args, cfg);
    check_snapshot_dir_escape(args, cfg, &dir)?;
    SnapshotStore::open_mut(dir).map_err(|e| CliError::Core(CoreError::Store(e)))
}

// Trade the store's opaque `path escape` for an actionable absolute-path hint.
pub fn check_snapshot_dir_escape(args: &GlobalArgs, cfg: &Config, dir: &Path) -> CliResult<()> {
    if !dir.components().any(|c| matches!(c, Component::ParentDir)) {
        return Ok(());
    }
    let configured = args
        .snapshot_dir_path
        .clone()
        .unwrap_or_else(|| cfg.defaults.snapshot_dir.clone());
    let root = if args.snapshot_dir_path.is_some() {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        cfg.config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    Err(CliError::SnapshotDirEscape {
        configured,
        resolved: dir.to_path_buf(),
        root,
    })
}
