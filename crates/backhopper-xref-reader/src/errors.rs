// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::ModuleName;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReadError {
    #[error("invalid utf-8 in {path:?} at byte offset {offset}")]
    InvalidUtf8 { path: PathBuf, offset: usize },

    #[error("duplicate module {name} declared in {first:?} and {second:?}")]
    DuplicateModule {
        name: ModuleName,
        first: PathBuf,
        second: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadWarning {
    EmptyFile {
        path: PathBuf,
    },
    NoModuleAttribute {
        path: PathBuf,
    },
    ConflictingModuleAttribute {
        path: PathBuf,
        first: ModuleName,
        second: ModuleName,
    },
    UnsupportedQuotedModuleName {
        path: PathBuf,
        raw: String,
    },
    AmbiguousApplicationAssignment {
        path: PathBuf,
        candidates: Vec<String>,
    },
    DuplicateModuleAcrossApplications {
        name: ModuleName,
        first_application: Option<String>,
        second_application: Option<String>,
    },
    UnresolvedInclude {
        path: PathBuf,
        target: PathBuf,
    },
}
