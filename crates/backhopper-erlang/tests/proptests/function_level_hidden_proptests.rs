// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_core::model::snapshot::Visibility;
use backhopper_erlang::ErlangExtractor;

fn form() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "-type descriptor() :: tuple().",
        "-opaque handle() :: reference().",
        "open(F) -> F.",
        "close(F) ->\n    ok.",
        "-spec open(term()) -> term().",
    ])
}

fn function_level_marker() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "-doc false.",
        "-doc hidden.",
        "-doc(hidden).",
        "%% @hidden",
        "%%@hidden",
    ])
}

proptest! {
    // a marker that documents one form never hides the module around it
    #[test]
    fn function_level_markers_leave_a_public_module_public(
        forms in prop::collection::vec((prop::option::of(function_level_marker()), form()), 1..10),
    ) {
        let mut src = String::from("-module(file).\n-export([open/1, close/1]).\n");
        for (marker, form) in &forms {
            if let Some(marker) = marker {
                src.push_str(marker);
                src.push('\n');
            }
            src.push_str(form);
            src.push('\n');
        }
        let m = ErlangExtractor::default().extract_module(&src).unwrap();
        prop_assert_eq!(m.visibility, Visibility::Public);
    }
}
