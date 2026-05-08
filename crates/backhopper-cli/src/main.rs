use std::process::ExitCode;

use backhopper_cli::run;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("backhopper: {e}");
            ExitCode::from(1)
        }
    }
}
