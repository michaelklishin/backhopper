// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Config, ConfigFile};
use backhopper_core::suites::ExtraRuleTrigger;

fn parse(toml_body: &str) -> Result<Config, backhopper_core::errors::ConfigError> {
    let raw: ConfigFile = toml::from_str(toml_body).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

#[test]
fn parses_suite_rule_with_string_include_suite() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
name = "schema_to_suite"
when_modified_path_matches = "^deps/(?P<plugin>[^/]+)/priv/schema/.*\\.schema$"
include_suite = "{plugin}_config_schema_SUITE"
also = "config_schema_SUITE"
"#;
    let cfg = parse(body).expect("parses");
    assert_eq!(cfg.suite_rules.len(), 1);
    let r = &cfg.suite_rules[0];
    assert_eq!(r.name, "schema_to_suite");
    match &r.trigger {
        ExtraRuleTrigger::PathRegex { captures, .. } => {
            assert_eq!(captures, &vec!["plugin".to_owned()]);
        }
        _ => panic!("expected PathRegex"),
    }
    assert_eq!(
        r.include_suite_templates,
        vec![
            "{plugin}_config_schema_SUITE".to_owned(),
            "config_schema_SUITE".to_owned(),
        ]
    );
}

#[test]
fn parses_suite_rule_with_array_include_suite_and_array_also() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "^test/(?P<dir>[^/]+)_SUITE_data/.*$"
include_suite = ["{dir}_SUITE", "{dir}_extra_SUITE"]
also = ["other_SUITE"]
"#;
    let cfg = parse(body).expect("parses");
    let r = &cfg.suite_rules[0];
    assert_eq!(r.include_suite_templates.len(), 3);
    assert!(r.name.starts_with("suite_rule_"));
}

#[test]
fn rejects_invalid_regex() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "[unclosed"
include_suite = "x_SUITE"
"#;
    let err = parse(body).unwrap_err();
    assert!(
        format!("{err}").contains("invalid regex"),
        "expected regex error, got: {err}"
    );
}

#[test]
fn rejects_template_referencing_unknown_capture() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "^deps/(?P<plugin>[^/]+)/.*$"
include_suite = "{nope}_SUITE"
"#;
    let err = parse(body).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unknown capture") && msg.contains("nope"),
        "expected unknown-placeholder error referencing 'nope', got: {msg}"
    );
}

#[test]
fn accepts_config_with_no_suite_rules() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
"#;
    let cfg = parse(body).unwrap();
    assert!(cfg.suite_rules.is_empty());
}
