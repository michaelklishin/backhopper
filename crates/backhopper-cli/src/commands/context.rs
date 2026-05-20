// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared command context: load config, open store.

use std::env;
use std::path::{Path, PathBuf};

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
    let cwd = PathBuf::from("./backhopper.toml");
    if cwd.exists() {
        return Ok(cwd);
    }
    tried.push(cwd);
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
    SnapshotStore::open(dir).map_err(|e| CliError::Core(CoreError::Store(e)))
}

pub fn open_store_mut(args: &GlobalArgs, cfg: &Config) -> CliResult<SnapshotStore<Mutable>> {
    let dir = snapshot_dir(args, cfg);
    SnapshotStore::open_mut(dir).map_err(|e| CliError::Core(CoreError::Store(e)))
}

pub fn snapshot_dir_for_path(p: &Path) -> PathBuf {
    p.to_path_buf()
}