use std::io;
use std::path::PathBuf;

use sysexits::ExitCode as SysExits;
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

    #[error("config not found: {0}")]
    ConfigNotFound(PathBuf),

    #[error("output error: {0}")]
    OutputError(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("{0}")]
    Other(String),
}

impl CliError {
    pub fn exit_code(&self) -> SysExits {
        match self {
            Self::Core(e) => map_core(e),
            Self::Io(_) => SysExits::IoErr,
            Self::ConfigNotFound(_) => SysExits::NoInput,
            Self::OutputError(_) => SysExits::IoErr,
            Self::InvalidInput(_) => SysExits::Usage,
            Self::Other(_) => SysExits::Software,
        }
    }
}

fn map_core(e: &CoreError) -> SysExits {
    match e {
        CoreError::Snapshot(_) => SysExits::DataErr,
        CoreError::Store(s) => match s {
            StoreError::SnapshotNotFound { .. } | StoreError::RootMissing(_) => SysExits::NoInput,
            _ => SysExits::IoErr,
        },
        CoreError::Config(c) => match c {
            ConfigError::NotFound(_) => SysExits::NoInput,
            _ => SysExits::DataErr,
        },
        CoreError::Git(_) => SysExits::IoErr,
        CoreError::Patch(_) => SysExits::DataErr,
        CoreError::Name(_) => SysExits::Usage,
        CoreError::Io(_) => SysExits::IoErr,
    }
}
