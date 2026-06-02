// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Walks a source tree directory and feeds files into an `XrefBuilder`.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use backhopper_core::ApplicationName;
use backhopper_xref::{Functions, ProjectLayout, Xref, XrefBuilder};

use crate::cli::TreeSource;
use crate::errors::{CliError, CliResult};

pub fn build_xref(tree: &TreeSource) -> CliResult<Xref<Functions>> {
    let dir = tree.tree_dir_path.as_path();
    if !dir.is_dir() {
        return Err(CliError::Other(format!(
            "--tree-dir-path {dir:?} is not a directory"
        )));
    }
    let layout = ProjectLayout::rabbitmq_main();
    let mut b = XrefBuilder::new().with_layout(layout);
    let app =
        ApplicationName::new("local".to_owned()).map_err(|e| CliError::Other(e.to_string()))?;
    let files = collect_erlang_files(dir).map_err(|e| CliError::Other(e.to_string()))?;
    b.add_application(app, files)
        .map_err(|e| CliError::Other(e.to_string()))?;
    let xref = b.build().map_err(|e| CliError::Other(e.to_string()))?;
    Ok(xref)
}

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "_build",
    "_rel",
    "logs",
    "node_modules",
    ".direnv",
    "target",
];

pub fn collect_erlang_files(root: &Path) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && SKIP_DIRS.contains(&name)
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if let Some(ext) = path.extension() {
                if ext == OsStr::new("erl") || ext == OsStr::new("hrl") {
                    let bytes = fs::read(&path)?;
                    out.push((path, bytes));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
