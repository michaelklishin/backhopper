// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::commands::snapshots::{CrossSeriesDiffPayload, DiffPayload, QualifiedFunArity};

fn payload(project: &str, from: &str, to: &str, removed: &[(&str, &str)]) -> DiffPayload {
    DiffPayload {
        project: project.into(),
        from: from.into(),
        to: to.into(),
        modules_added: Vec::new(),
        modules_removed: Vec::new(),
        exports_added: Vec::new(),
        exports_removed: removed
            .iter()
            .map(|(m, fa)| QualifiedFunArity {
                module: (*m).into(),
                fun_arity: (*fa).into(),
            })
            .collect(),
        types_added: Vec::new(),
        types_removed: Vec::new(),
        callbacks_added: Vec::new(),
        callbacks_removed: Vec::new(),
        headers_added: Vec::new(),
        headers_removed: Vec::new(),
        records_added: Vec::new(),
        records_removed: Vec::new(),
    }
}

#[test]
fn cross_series_payload_carries_one_diff_per_project() {
    let cross = CrossSeriesDiffPayload {
        from_series: "rabbitmq-4.1".into(),
        to_series: "rabbitmq-4.2".into(),
        projects: vec![
            payload("ra", "v2.16.13", "v2.17.3", &[("ra_server", "metrics/1")]),
            payload("osiris", "v1.8.8", "v1.10.3", &[]),
        ],
    };
    assert_eq!(cross.projects.len(), 2);
    assert_eq!(cross.projects[0].project, "ra");
    assert_eq!(cross.projects[1].project, "osiris");
}

#[test]
fn cross_series_payload_can_omit_projects_present_in_only_one_series() {
    // payload holds whatever the caller computed: serialization stays faithful.
    let cross = CrossSeriesDiffPayload {
        from_series: "a".into(),
        to_series: "b".into(),
        projects: vec![payload("ra", "v1", "v2", &[])],
    };
    let json = serde_json::to_string(&cross).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["projects"].as_array().unwrap().len(), 1);
}

#[test]
fn cross_series_payload_serializes_json_with_per_project_blocks() {
    let cross = CrossSeriesDiffPayload {
        from_series: "rabbitmq-4.1".into(),
        to_series: "rabbitmq-4.2".into(),
        projects: vec![payload("ra", "v2.16.13", "v2.17.3", &[])],
    };
    let json = serde_json::to_string(&cross).unwrap();
    assert!(json.contains("\"from_series\":\"rabbitmq-4.1\""), "{json}");
    assert!(json.contains("\"to_series\":\"rabbitmq-4.2\""), "{json}");
    assert!(json.contains("\"projects\""), "{json}");
    assert!(json.contains("\"project\":\"ra\""), "{json}");
}
