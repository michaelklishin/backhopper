use std::path::PathBuf;

use sysexits::ExitCode as SysExits;
use thiserror::Error;

pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] backhopper_core::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

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

fn map_core(e: &backhopper_core::Error) -> SysExits {
    use backhopper_core::Error::*;
    match e {
        Snapshot(_) => SysExits::DataErr,
        Store(s) => match s {
            backhopper_core::errors::StoreError::SnapshotNotFound { .. } => SysExits::NoInput,
            backhopper_core::errors::StoreError::RootMissing(_) => SysExits::NoInput,
            _ => SysExits::IoErr,
        },
        Config(c) => match c {
            backhopper_core::errors::ConfigError::NotFound(_) => SysExits::NoInput,
            _ => SysExits::DataErr,
        },
        Git(_) => SysExits::IoErr,
        Patch(_) => SysExits::DataErr,
        Name(_) => SysExits::Usage,
        Io(_) => SysExits::IoErr,
    }
}
