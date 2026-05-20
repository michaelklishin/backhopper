// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::HashSet;

use backhopper_core::compat::otp::{allowlist, is_otp_module};
use backhopper_core::model::names::ModuleName;

#[test]
fn allowlist_has_no_duplicates() {
    let names = allowlist();
    let set: HashSet<&&str> = names.iter().collect();
    assert_eq!(set.len(), names.len(), "duplicate entries in OTP allowlist");
}

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
        assert!(is_otp_module(&m), "{} should be OTP", name);
    }
}

#[test]
fn project_modules_are_not_otp() {
    for name in ["ra", "khepri", "rabbit_misc", "cowboy_req", "ranch"] {
        let m = ModuleName::new(name).unwrap();
        assert!(!is_otp_module(&m), "{} should not be OTP", name);
    }
}
