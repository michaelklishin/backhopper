// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A `%` comment line inside an `-export` list changes nothing the
//! surface answers, whatever the comment says.

use std::str::FromStr;

use proptest::prelude::*;

use backhopper_core::compat::source_attributes::extract_exports;
use backhopper_core::model::names::{Arity, FunctionName};

fn entry() -> impl Strategy<Value = (String, u8)> {
    (
        prop::sample::select(vec![
            "init",
            "get_pid",
            "get_name",
            "make_enqueue",
            "handle_aux",
            "query_stat",
        ]),
        0u8..6,
    )
        .prop_map(|(name, arity)| (name.to_owned(), arity))
}

fn comment() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "%% protocol helpers",
        "% pid",
        "% name (#resource)",
        "% see ?MODULE",
        "% [deprecated]",
        "%% a, b, c",
    ])
    .prop_map(str::to_owned)
}

fn export_list(entries: &[(String, u8)], comments: &[Option<String>]) -> String {
    let mut body = String::from("-export([\n");
    for (i, (name, arity)) in entries.iter().enumerate() {
        if let Some(Some(line)) = comments.get(i) {
            body.push_str(&format!("         {line}\n"));
        }
        let comma = if i + 1 < entries.len() { "," } else { "" };
        body.push_str(&format!("         {name}/{arity}{comma}\n"));
    }
    body.push_str("        ]).\n");
    body
}

proptest! {
    #[test]
    fn comment_lines_do_not_change_the_surface(
        entries in prop::collection::vec(entry(), 1..8),
        comments in prop::collection::vec(prop::option::of(comment()), 8),
    ) {
        let plain = extract_exports(&export_list(&entries, &[]));
        let commented = extract_exports(&export_list(&entries, &comments));
        prop_assert_eq!(plain.is_complete(), commented.is_complete());
        for (name, arity) in &entries {
            let key = (FunctionName::from_str(name).unwrap(), Arity::new(*arity));
            prop_assert_eq!(plain.lookup(&key), commented.lookup(&key));
        }
    }
}
