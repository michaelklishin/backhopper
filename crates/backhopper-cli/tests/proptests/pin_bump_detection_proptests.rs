// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Detection duality: render a pin map and a set of edits to a
//! unified diff, detect, and recover exactly the edit set. Unchanged
//! pins ride as context lines, so this also pins "context never
//! produces a bump" structurally.

use std::collections::BTreeMap;

use proptest::prelude::*;

use backhopper_cli::commands::rabbitmq_components::detect_pin_bumps;
use backhopper_core::compat::patch::Patch;

#[derive(Debug, Clone)]
enum Edit {
    Bump { to: String },
    Remove,
}

fn version() -> impl Strategy<Value = String> {
    ("[0-9]{1,2}", "[0-9]{1,2}", "[0-9]{1,2}").prop_map(|(a, b, c)| format!("{a}.{b}.{c}"))
}

// distinct dep names with a version each; a parallel edit decision per name
fn scenario() -> impl Strategy<
    Value = (
        BTreeMap<String, String>,
        BTreeMap<String, Option<Edit>>,
        Vec<(String, String)>,
    ),
> {
    let base = proptest::collection::btree_map("[a-z]{1,8}", version(), 0..6);
    base.prop_flat_map(|base| {
        let names: Vec<String> = base.keys().cloned().collect();
        let edits = names
            .iter()
            .map(|n| {
                let n = n.clone();
                prop_oneof![
                    2 => Just(None),
                    2 => version().prop_map(|v| Some(Edit::Bump { to: v })),
                    1 => Just(Some(Edit::Remove)),
                ]
                .prop_map(move |e| (n.clone(), e))
            })
            .collect::<Vec<_>>();
        let additions = proptest::collection::vec(("[a-z]{9,12}", version()), 0..3);
        (Just(base), edits, additions)
    })
    .prop_map(|(base, edits, additions)| (base, edits.into_iter().collect(), additions))
}

fn render_patch(
    base: &BTreeMap<String, String>,
    edits: &BTreeMap<String, Option<Edit>>,
    additions: &[(String, String)],
) -> String {
    let mut body = String::new();
    let mut old = 0usize;
    let mut new = 0usize;
    for (name, from) in base {
        match edits.get(name).and_then(Option::as_ref) {
            None => {
                body.push_str(&format!(" dep_{name} = hex {from}\n"));
                old += 1;
                new += 1;
            }
            Some(Edit::Bump { to }) => {
                body.push_str(&format!("-dep_{name} = hex {from}\n"));
                body.push_str(&format!("+dep_{name} = hex {to}\n"));
                old += 1;
                new += 1;
            }
            Some(Edit::Remove) => {
                body.push_str(&format!("-dep_{name} = hex {from}\n"));
                old += 1;
            }
        }
    }
    for (name, version) in additions {
        body.push_str(&format!("+dep_{name} = hex {version}\n"));
        new += 1;
    }
    format!(
        "diff --git a/rabbitmq-components.mk b/rabbitmq-components.mk\n\
         --- a/rabbitmq-components.mk\n\
         +++ b/rabbitmq-components.mk\n\
         @@ -1,{old} +1,{new} @@\n{body}"
    )
}

proptest! {
    #[test]
    fn detection_recovers_exactly_the_edit_set((base, edits, additions) in scenario()) {
        let text = render_patch(&base, &edits, &additions);
        let patch = Patch::parse(text.as_bytes()).expect("synthesized patch parses");
        let bumps = detect_pin_bumps(&patch.files);

        // expected: every Bump whose target version differs, plus every addition
        let mut expected: BTreeMap<String, (Option<String>, String)> = BTreeMap::new();
        for (name, edit) in &edits {
            if let Some(Edit::Bump { to }) = edit {
                let from = &base[name];
                if from != to {
                    expected.insert(
                        name.clone(),
                        (Some(format!("hex {from}")), format!("hex {to}")),
                    );
                }
            }
        }
        for (name, version) in &additions {
            expected.insert(name.clone(), (None, format!("hex {version}")));
        }

        let detected: BTreeMap<String, (Option<String>, String)> = bumps
            .into_iter()
            .map(|b| (b.dep.as_str().to_owned(), (b.from, b.to)))
            .collect();
        prop_assert_eq!(detected, expected);
    }
}
