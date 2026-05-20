// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::process;

use bel7_cli::{ExitCodeExt, print_error};

use backhopper_cli::run;

fn main() -> process::ExitCode {
    match run() {
        Ok(code) => process::ExitCode::from(code as u8),
        Err(e) => {
            print_error(format!("backhopper: {e}"));
            process::ExitCode::from(e.exit_code().to_i32() as u8)
        }
    }
}