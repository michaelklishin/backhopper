use std::path::PathBuf;

use sysexits::ExitCode;

use backhopper_cli::CliError;

#[test]
fn config_not_found_maps_to_no_input() {
    let e = CliError::ConfigNotFound {
        tried: vec![PathBuf::from("/x")],
    };
    assert_eq!(e.exit_code(), ExitCode::NoInput);
}

#[test]
fn config_not_found_lists_every_tried_path_in_message() {
    let e = CliError::ConfigNotFound {
        tried: vec![
            PathBuf::from("./backhopper.toml"),
            PathBuf::from("/home/me/.config/backhopper/backhopper.toml"),
        ],
    };
    let msg = e.to_string();
    assert!(msg.contains("./backhopper.toml"), "msg was: {}", msg);
    assert!(
        msg.contains("/home/me/.config/backhopper/backhopper.toml"),
        "msg was: {}",
        msg
    );
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
