// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::model::verdict::{FileKind, TouchedKinds};

fn arb_path() -> impl Strategy<Value = PathBuf> {
    "[a-zA-Z0-9_./-]{0,80}".prop_map(PathBuf::from)
}

proptest! {
    #[test]
    fn classify_never_panics(p in arb_path()) {
        let _ = TouchedKinds::classify(&p);
    }

    #[test]
    fn from_paths_totals_match_input_length(
        paths in proptest::collection::vec(arb_path(), 0..30)
    ) {
        let tk = TouchedKinds::from_paths(&paths);
        let total = tk.erl
            + tk.hrl
            + tk.schema
            + tk.docs
            + tk.tests
            + tk.makefile
            + tk.mix_exs
            + tk.ci_workflow
            + tk.app_src
            + tk.rebar_config
            + tk.other;
        prop_assert_eq!(total as usize, paths.len());
    }

    #[test]
    fn inapplicable_reason_is_none_iff_any_erl_or_hrl(
        paths in proptest::collection::vec(arb_path(), 0..30)
    ) {
        let tk = TouchedKinds::from_paths(&paths);
        let any_source = paths.iter().any(|p| {
            matches!(TouchedKinds::classify(p), FileKind::Erl | FileKind::Hrl)
        });
        prop_assert_eq!(tk.inapplicable_reason().is_none(), any_source);
    }
}
