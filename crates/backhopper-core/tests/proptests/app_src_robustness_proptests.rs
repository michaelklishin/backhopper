// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use proptest::prelude::*;

use backhopper_core::app_src::parse;

proptest! {
    /// The parser must never panic on arbitrary UTF-8 text: malformed
    /// input returns `Err`, not unwinds.
    #[test]
    fn parser_does_not_panic_on_arbitrary_input(text in "[ -~\n]{0,200}") {
        let _ = parse(Path::new("p.app.src"), &text);
    }

    /// Plain garbage strings always return Err; this is a sanity check
    /// that the parser actually validates, not silently returns Ok.
    #[test]
    fn random_short_garbage_is_rejected(garbage in "[a-zA-Z0-9]{0,30}") {
        let result = parse(Path::new("p.app.src"), &garbage);
        prop_assert!(result.is_err());
    }
}