// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! EEP-66 sigil strings inside attribute bodies: the region must not
//! swallow the attributes after it.

use backhopper_erlang::tokenizer::iterate_attributes;

#[test]
fn a_define_with_a_paren_sigil_body_leaves_the_next_attribute_intact() {
    let src = "-define(GREETING, ~s(a \" b)).\n-export([start/0]).\n";
    let attrs = iterate_attributes(src);
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[0].name, "define");
    assert_eq!(attrs[0].body, "(GREETING, ~s(a \" b))");
    assert_eq!(attrs[1].name, "export");
    assert_eq!(attrs[1].body, "([start/0])");
}

#[test]
fn a_moduledoc_triple_quoted_sigil_leaves_the_next_attribute_intact() {
    let src = "-moduledoc ~S\"\"\"\nProse with \" quotes and full stops.\n-export([phantom/0]).\n\"\"\".\n-export([start/0, stop/1]).\n";
    let attrs = iterate_attributes(src);
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[0].name, "moduledoc");
    let export = &attrs[1];
    assert_eq!(export.name, "export");
    assert_eq!(export.body, "([start/0, stop/1])");
    assert_eq!(export.line, 5);
}

#[test]
fn a_verbatim_sigil_ending_in_a_backslash_closes_at_its_quote() {
    let src = "-define(P, ~S\"a\\\").\n-export([go/0]).\n";
    let attrs = iterate_attributes(src);
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[1].name, "export");
}
