// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_erlang::tokenizer::iterate_attributes;

proptest! {
    #[test]
    fn iterate_does_not_panic(s in "[\\PC]{0,512}") {
        let _ = iterate_attributes(&s);
    }

    #[test]
    fn iterate_picks_up_well_formed_module_attribute(name in "[a-z][a-z0-9_]{0,8}") {
        let src = format!("-module({name}).\n");
        let blocks = iterate_attributes(&src);
        prop_assert_eq!(blocks.len(), 1);
        prop_assert_eq!(&blocks[0].name, "module");
    }
}
