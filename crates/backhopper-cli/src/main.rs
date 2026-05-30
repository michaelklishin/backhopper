// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::process;

use bel7_cli::{Outcome, print_error};

use backhopper_cli::run;

fn main() -> process::ExitCode {
    let outcome = run();
    if let Outcome::Failure(e) = &outcome {
        print_error(format!("backhopper: {e}"));
        if let Some(detail) = e.detail() {
            eprintln!("{detail}");
        }
        if let Some(hint) = e.hint() {
            eprintln!("hint: {hint}");
        }
    }
    let code = u8::try_from(outcome.exit_code_i32()).unwrap_or(u8::MAX);
    process::ExitCode::from(code)
}
