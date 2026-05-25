// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Properties of the additive merge:
//!
//!  * applying the same payload twice never changes the second result
//!  * the merged text always parses as TOML again
//!  * no existing pin gets dropped

use std::collections::BTreeSet;

use proptest::prelude::*;

use backhopper_cli::commands::series::{PinPayload, SyncOutput, merge_sync_into_config_text};

fn pin_payload() -> impl Strategy<Value = PinPayload> {
    (
        "[a-z][a-z0-9_]{0,12}",
        "v[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
    )
        .prop_map(|(project, tag)| PinPayload { project, tag })
}

fn dedup_by_project(pins: Vec<PinPayload>) -> Vec<PinPayload> {
    let mut seen = BTreeSet::new();
    pins.into_iter()
        .filter(|p| seen.insert(p.project.clone()))
        .collect()
}

fn render_pins(pins: &[PinPayload]) -> String {
    if pins.is_empty() {
        return "pins = []".into();
    }
    let mut out = String::from("pins = [\n");
    for p in pins {
        out.push_str(&format!(
            "    {{ project = \"{}\", tag = \"{}\" }},\n",
            p.project, p.tag
        ));
    }
    out.push(']');
    out
}

fn config_with(existing: &[PinPayload]) -> String {
    let mut projects = String::new();
    let mut seen = BTreeSet::new();
    for p in existing {
        if seen.insert(p.project.clone()) {
            projects.push_str(&format!(
                "[[project]]\nname = \"{}\"\ngit_url = \"/tmp/x.git\"\n\n",
                p.project
            ));
        }
    }
    format!(
        "config_version = 1\n\n{projects}[[series]]\nname = \"rabbitmq-x\"\n{}\n",
        render_pins(existing)
    )
}

proptest! {
    #[test]
    fn merge_is_idempotent_on_second_apply(
        existing in prop::collection::vec(pin_payload(), 0..6),
        inferred in prop::collection::vec(pin_payload(), 0..6),
    ) {
        let existing = dedup_by_project(existing);
        let inferred = dedup_by_project(inferred);
        let cfg = config_with(&existing);
        let payload = SyncOutput {
            name: "rabbitmq-x".into(),
            pins: inferred,
            dropped_unconfigured: Vec::new(),
        };
        let (once, _) = merge_sync_into_config_text(&cfg, &payload, false).unwrap();
        let (twice, second_outcome) = merge_sync_into_config_text(&once, &payload, false).unwrap();
        prop_assert_eq!(once, twice);
        prop_assert!(second_outcome.added.is_empty());
        prop_assert!(second_outcome.updated.is_empty());
    }

    #[test]
    fn merge_never_drops_existing_project(
        existing in prop::collection::vec(pin_payload(), 1..6),
        inferred in prop::collection::vec(pin_payload(), 0..6),
    ) {
        let existing = dedup_by_project(existing);
        let inferred = dedup_by_project(inferred);
        let cfg = config_with(&existing);
        let payload = SyncOutput {
            name: "rabbitmq-x".into(),
            pins: inferred,
            dropped_unconfigured: Vec::new(),
        };
        let (out, _) = merge_sync_into_config_text(&cfg, &payload, false).unwrap();
        for pin in &existing {
            prop_assert!(
                out.contains(&format!("project = \"{}\"", pin.project)),
                "lost existing project {}",
                pin.project
            );
        }
    }
}
