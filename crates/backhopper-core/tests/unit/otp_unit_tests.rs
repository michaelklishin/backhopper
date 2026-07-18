// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::otp::is_otp_module;
use backhopper_core::model::names::ModuleName;

#[test]
fn well_known_modules_are_otp() {
    for name in [
        "lists",
        "maps",
        "gen_server",
        "ets",
        "application",
        "crypto",
        "ssl",
        "rand",
        "logger",
        "mnesia",
    ] {
        let m = ModuleName::new(name).unwrap();
        assert!(is_otp_module(&m), "{name} should be OTP");
    }
}

#[test]
fn project_modules_are_not_otp() {
    for name in ["ra", "khepri", "rabbit_misc", "cowboy_req", "ranch"] {
        let m = ModuleName::new(name).unwrap();
        assert!(!is_otp_module(&m), "{name} should not be OTP");
    }
}
