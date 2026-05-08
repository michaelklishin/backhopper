use std::path::PathBuf;

use sysexits::ExitCode;

use backhopper_cli::CliError;

#[test]
fn config_not_found_maps_to_no_input() {
    let e = CliError::ConfigNotFound(PathBuf::from("/x"));
    assert_eq!(e.exit_code(), ExitCode::NoInput);
}

#[test]
fn invalid_input_maps_to_usage() {
    let e = CliError::InvalidInput("bad".into());
    assert_eq!(e.exit_code(), ExitCode::Usage);
}

#[test]
fn io_error_maps_to_io_err() {
    let e = CliError::Io(std::io::Error::other("boom"));
    assert_eq!(e.exit_code(), ExitCode::IoErr);
}
