// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The apps a host release is rooted at: the starting set whose closure
//! over the app manifests is what the release ships.

use std::str::FromStr;

use serde::Serialize;
use thiserror::Error;

use crate::erlang_mk::MakeVariables;
use crate::model::app_manifest::ManifestUnreadable;
use crate::model::names::{ApplicationName, MakeVariableName, RelativePath};
use crate::model::non_empty::NonEmpty;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MakefileVariable {
    pub file_path: RelativePath,
    pub variable: MakeVariableName,
}

/// At least one root between the literal list and the variable: an enum
/// rather than two optional fields, so "no roots" is not a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ReleaseRoots {
    Apps(NonEmpty<ApplicationName>),
    Variable(MakefileVariable),
    Both {
        apps: NonEmpty<ApplicationName>,
        variable: MakefileVariable,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReleaseRootsError {
    #[error("{file_path} is not present at the tag")]
    FileMissing { file_path: RelativePath },

    #[error("{variable} in {file_path} cannot be read: {reason:?}")]
    VariableUnreadable {
        file_path: RelativePath,
        variable: MakeVariableName,
        reason: ManifestUnreadable,
    },

    #[error("{variable} in {file_path} lists no apps and no literal roots are configured")]
    NoRoots {
        file_path: RelativePath,
        variable: MakeVariableName,
    },

    #[error("{word:?} in {variable} is not an app name")]
    NotAnApp {
        variable: MakeVariableName,
        word: String,
    },
}

impl ReleaseRoots {
    pub fn read(
        &self,
        read_file: impl Fn(&RelativePath) -> Option<String>,
    ) -> Result<NonEmpty<ApplicationName>, ReleaseRootsError> {
        match self {
            Self::Apps(apps) => Ok(apps.clone()),
            Self::Variable(variable) => {
                let words = variable_roots(variable, &read_file)?;
                NonEmpty::try_from(words).map_err(|_| ReleaseRootsError::NoRoots {
                    file_path: variable.file_path.clone(),
                    variable: variable.variable.clone(),
                })
            }
            Self::Both { apps, variable } => {
                let more = variable_roots(variable, &read_file)?;
                Ok(NonEmpty::new(
                    apps.first().clone(),
                    apps.iter().skip(1).cloned().chain(more),
                ))
            }
        }
    }
}

fn variable_roots(
    variable: &MakefileVariable,
    read_file: &impl Fn(&RelativePath) -> Option<String>,
) -> Result<Vec<ApplicationName>, ReleaseRootsError> {
    let text = read_file(&variable.file_path).ok_or_else(|| ReleaseRootsError::FileMissing {
        file_path: variable.file_path.clone(),
    })?;
    let variables = MakeVariables::parse(&text);
    let words = variables.words(&variable.variable).map_err(|reason| {
        ReleaseRootsError::VariableUnreadable {
            file_path: variable.file_path.clone(),
            variable: variable.variable.clone(),
            reason,
        }
    })?;
    words
        .iter()
        .map(|word| {
            ApplicationName::from_str(word).map_err(|_| ReleaseRootsError::NotAnApp {
                variable: variable.variable.clone(),
                word: word.clone(),
            })
        })
        .collect()
}
