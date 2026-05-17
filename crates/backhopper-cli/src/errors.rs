//! CLI error type. Maps to BSD `sysexits` via `bel7_cli::ExitCodeProvider`.

use std::io;
use std::path::PathBuf;

use bel7_cli::{ExitCode, ExitCodeProvider, codes};
use thiserror::Error;

use backhopper_core::Error as CoreError;
use backhopper_core::errors::{ConfigError, StoreError};

pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] CoreError),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("config not found; tried (in order): {}", tried_display(.tried))]
    ConfigNotFound { tried: Vec<PathBuf> },

    #[error("output error: {0}")]
    OutputError(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("{0}")]
    Other(String),
}

impl ExitCodeProvider for CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Core(e) => map_core(e),
            Self::Io(_) => codes::IO_ERR,
            Self::ConfigNotFound { .. } => codes::NO_INPUT,
            Self::OutputError(_) => codes::IO_ERR,
            Self::InvalidInput(_) => codes::USAGE,
            Self::Other(_) => codes::SOFTWARE,
        }
    }
}

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        <Self as ExitCodeProvider>::exit_code(self)
    }
}

fn tried_display(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn map_core(e: &CoreError) -> ExitCode {
    match e {
        CoreError::Snapshot(_) => codes::DATA_ERR,
        CoreError::Store(s) => match s {
            StoreError::SnapshotNotFound { .. } | StoreError::RootMissing(_) => codes::NO_INPUT,
            _ => codes::IO_ERR,
        },
        CoreError::Config(c) => match c {
            ConfigError::NotFound(_) => codes::NO_INPUT,
            _ => codes::DATA_ERR,
        },
        CoreError::Git(_) => codes::IO_ERR,
        CoreError::Patch(_) => codes::DATA_ERR,
        CoreError::Name(_) => codes::USAGE,
        CoreError::Io(_) => codes::IO_ERR,
    }
}
