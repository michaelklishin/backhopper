// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Property tests for `build_suggestions`: invariants over arbitrary
//! diagnostics. The function must never panic and never propose a project
//! the user already has.

use proptest::prelude::*;

use backhopper_core::compat::is_otp_module;
use backhopper_core::model::names::{ModuleName, ProjectName};
use backhopper_core::model::verdict::Diagnostics;

use backhopper_cli::commands::suggest::build_suggestions;

fn module_strategy() -> impl Strategy<Value = String> {
    // restrict to valid module-name characters so the strategy almost always parses as ModuleName
    "[a-z][a-z0-9_]{0,15}".prop_filter("non-empty", |s| !s.is_empty())
}

fn diagnostics_strategy() -> impl Strategy<Value = Diagnostics> {
    proptest::collection::vec((module_strategy(), 1u32..50u32), 0..20).prop_map(|pairs| {
        let mut d = Diagnostics::default();
        for (m, c) in pairs {
            if let Ok(name) = ModuleName::new(&m) {
                d.untracked_calls.insert(name, c as usize);
            }
        }
        d
    })
}

fn known_projects_strategy() -> impl Strategy<Value = Vec<ProjectName>> {
    proptest::collection::vec(
        "[a-z][a-z0-9]{0,8}".prop_filter_map("valid project name", |s| ProjectName::new(s).ok()),
        0..5,
    )
}

proptest! {
    #[test]
    fn build_suggestions_never_panics(d in diagnostics_strategy(), known in known_projects_strategy()) {
        let _ = build_suggestions(&d, &known);
    }

    #[test]
    fn known_projects_are_never_suggested(d in diagnostics_strategy(), known in known_projects_strategy()) {
        let suggestions = build_suggestions(&d, &known);
        for s in &suggestions {
            for k in &known {
                prop_assert_ne!(s.name.as_str(), k.as_str());
            }
        }
    }

    #[test]
    fn otp_modules_never_contribute(d in diagnostics_strategy()) {
        let suggestions = build_suggestions(&d, &[]);
        for s in &suggestions {
            for m in &s.modules {
                if let Ok(name) = ModuleName::new(m) {
                    prop_assert!(!is_otp_module(&name));
                }
            }
        }
    }

    #[test]
    fn suggestions_are_sorted_by_call_sites_desc(d in diagnostics_strategy()) {
        let suggestions = build_suggestions(&d, &[]);
        for w in suggestions.windows(2) {
            prop_assert!(w[0].call_sites >= w[1].call_sites);
        }
    }
}
