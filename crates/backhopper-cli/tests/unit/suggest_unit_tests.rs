// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Tests for the project-suggestion helper used by `check --suggest-projects`.

use backhopper_core::model::names::{ModuleName, ProjectName};
use backhopper_core::model::verdict::Diagnostics;

use backhopper_cli::commands::suggest::{
    ProjectSuggestion, append_suggestions_to_config, build_suggestions, render_suggestion,
};

fn diag(modules: &[(&str, usize)]) -> Diagnostics {
    let mut d = Diagnostics::default();
    for (m, c) in modules {
        d.untracked_calls.insert(ModuleName::new(*m).unwrap(), *c);
    }
    d
}

fn project(name: &str) -> ProjectName {
    ProjectName::new(name).unwrap()
}

#[test]
fn empty_diagnostics_yield_no_suggestions() {
    let suggestions = build_suggestions(&Diagnostics::default(), &[]);
    assert!(suggestions.is_empty());
}

#[test]
fn otp_modules_are_dropped() {
    let d = diag(&[("lists", 10), ("gen_server", 5)]);
    let suggestions = build_suggestions(&d, &[]);
    assert!(
        suggestions.is_empty(),
        "expected no suggestions, got {suggestions:?}"
    );
}

#[test]
fn already_known_projects_are_skipped() {
    let d = diag(&[("rabbit_misc", 3), ("rabbit_amqp_util", 7)]);
    let known = vec![project("rabbit")];
    let suggestions = build_suggestions(&d, &known);
    assert!(suggestions.is_empty());
}

#[test]
fn modules_without_underscore_are_dropped() {
    let d = diag(&[("cowboy", 2)]);
    let suggestions = build_suggestions(&d, &[]);
    assert!(
        suggestions.is_empty(),
        "bare module names cannot be split into a project prefix"
    );
}

#[test]
fn suggestions_are_grouped_by_prefix() {
    let d = diag(&[
        ("rabbit_misc", 3),
        ("rabbit_amqp_util", 7),
        ("ranch_listener", 1),
    ]);
    let suggestions = build_suggestions(&d, &[]);
    assert_eq!(suggestions.len(), 2);
    let rabbit = suggestions.iter().find(|s| s.name == "rabbit").unwrap();
    assert_eq!(rabbit.call_sites, 10);
    assert_eq!(rabbit.modules.len(), 2);
    let ranch = suggestions.iter().find(|s| s.name == "ranch").unwrap();
    assert_eq!(ranch.call_sites, 1);
}

#[test]
fn suggestions_sort_by_call_sites_descending_then_name() {
    let d = diag(&[("alpha_one", 2), ("bravo_one", 5), ("charlie_one", 5)]);
    let suggestions = build_suggestions(&d, &[]);
    assert_eq!(suggestions.len(), 3);
    assert_eq!(suggestions[0].name, "bravo");
    assert_eq!(suggestions[1].name, "charlie");
    assert_eq!(suggestions[2].name, "alpha");
}

#[test]
fn module_list_is_sorted_alphabetically() {
    let d = diag(&[
        ("rabbit_zoo", 1),
        ("rabbit_amqp_util", 1),
        ("rabbit_misc", 4),
    ]);
    let suggestions = build_suggestions(&d, &[]);
    let rabbit = &suggestions[0];
    assert_eq!(
        rabbit.modules,
        vec!["rabbit_amqp_util", "rabbit_misc", "rabbit_zoo"]
    );
}

#[test]
fn render_suggestion_produces_a_toml_stub_with_summary_comment() {
    let s = ProjectSuggestion {
        name: "rabbit".into(),
        modules: vec!["rabbit_amqp_util".into(), "rabbit_misc".into()],
        call_sites: 12,
    };
    let mut out: Vec<u8> = Vec::new();
    render_suggestion(&mut out, &s, 5).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("# 12 untracked calls"));
    assert!(text.contains("rabbit_amqp_util"));
    assert!(text.contains("[[project]]"));
    assert!(text.contains("name       = \"rabbit\""));
    assert!(text.contains("git_url"));
}

#[test]
fn render_suggestion_caps_module_preview() {
    let s = ProjectSuggestion {
        name: "rabbit".into(),
        modules: (0..10).map(|i| format!("rabbit_m{i}")).collect(),
        call_sites: 100,
    };
    let mut out: Vec<u8> = Vec::new();
    render_suggestion(&mut out, &s, 3).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("(+7 more)"));
}

#[test]
fn append_to_empty_config_creates_project_blocks() {
    let starter = "config_version = 1\n";
    let suggestions = vec![ProjectSuggestion {
        name: "rabbit".into(),
        modules: vec!["rabbit_misc".into()],
        call_sites: 1,
    }];
    let updated = append_suggestions_to_config(starter, &suggestions).unwrap();
    assert!(updated.contains("[[project]]"));
    assert!(updated.contains("\"rabbit\""));
    assert!(updated.contains("TODO"));
}

#[test]
fn appending_a_known_project_is_a_no_op() {
    let starter = r#"
config_version = 1

[[project]]
name = "rabbit"
git_url = "/tmp/rabbit.git"
"#;
    let suggestions = vec![ProjectSuggestion {
        name: "rabbit".into(),
        modules: vec![],
        call_sites: 0,
    }];
    let updated = append_suggestions_to_config(starter, &suggestions).unwrap();
    let count = updated.matches("[[project]]").count();
    assert_eq!(count, 1, "should not add a second rabbit block");
}

#[test]
fn append_rejects_malformed_toml() {
    let err = append_suggestions_to_config("not = valid = toml", &[]).unwrap_err();
    assert!(err.contains("invalid TOML"));
}

#[test]
fn rendered_stub_parses_as_loadable_config_when_wrapped() {
    let s = ProjectSuggestion {
        name: "rabbit".into(),
        modules: vec!["rabbit_misc".into()],
        call_sites: 4,
    };
    let mut out: Vec<u8> = Vec::new();
    render_suggestion(&mut out, &s, 5).unwrap();
    let stub = String::from_utf8(out).unwrap();
    let body = format!("config_version = 1\n\n{stub}");
    let parsed: toml::Value = toml::from_str(&body).expect("rendered stub must be valid TOML");
    let projects = parsed
        .get("project")
        .and_then(|v| v.as_array())
        .expect("[[project]] array");
    assert_eq!(projects.len(), 1);
    let first = projects[0].as_table().unwrap();
    assert!(
        first.contains_key("git_url"),
        "git_url is required, not commented out"
    );
}

#[test]
fn append_then_re_append_is_idempotent() {
    let suggestions = vec![ProjectSuggestion {
        name: "rabbit".into(),
        modules: vec!["rabbit_misc".into()],
        call_sites: 1,
    }];
    let once = append_suggestions_to_config("config_version = 1\n", &suggestions).unwrap();
    let twice = append_suggestions_to_config(&once, &suggestions).unwrap();
    assert_eq!(once, twice);
}
