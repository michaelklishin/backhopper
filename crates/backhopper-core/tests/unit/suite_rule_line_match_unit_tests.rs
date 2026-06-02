// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Config, ConfigFile};
use backhopper_core::errors::ConfigError;

fn parse(toml_body: &str) -> Result<Config, ConfigError> {
    let raw: ConfigFile = toml::from_str(toml_body).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

#[test]
fn parses_when_modified_line_matches_with_named_capture() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
name = "dep_bump_sweep"
when_modified_path_matches = "(^|/)rabbitmq-components\\.mk$"
when_modified_line_matches = "^dep_(?P<dep>[a-z_]+)\\s*=\\s*hex\\s+"
also = ["{dep}_datatype_migration_SUITE"]
include_suite_for_dep_modules = true
"#;
    let cfg = parse(body).expect("parses");
    let r = &cfg.suite_rules[0];
    assert!(r.include_suite_for_dep_modules);
    let lm = r.line_match.as_ref().expect("line_match present");
    assert_eq!(lm.captures, vec!["dep".to_owned()]);
    assert_eq!(
        r.include_suite_templates,
        vec!["{dep}_datatype_migration_SUITE"]
    );
}

#[test]
fn templates_resolve_against_line_captures() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "(^|/)rabbitmq-components\\.mk$"
when_modified_line_matches = "^dep_(?P<dep>[a-z_]+)\\s*=\\s*hex\\s+"
also = ["{dep}_extra_SUITE"]
"#;
    let _cfg = parse(body).expect("template referencing line capture parses");
}

#[test]
fn rejects_template_referencing_unknown_line_capture() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "(^|/)rabbitmq-components\\.mk$"
when_modified_line_matches = "^dep_(?P<dep>[a-z_]+)\\s*=\\s*hex\\s+"
also = ["{nope}_SUITE"]
"#;
    let err = parse(body).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("nope"), "expected 'nope' in error, got: {msg}");
}

#[test]
fn rejects_dep_modules_without_dep_capture() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "(^|/)Makefile$"
include_suite_for_dep_modules = true
"#;
    let err = parse(body).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("requires a named capture `dep`"),
        "expected dep-capture error, got: {msg}"
    );
}

#[test]
fn dep_capture_from_path_satisfies_the_rule() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "(^|/)deps/(?P<dep>[a-z_]+)/Makefile$"
include_suite_for_dep_modules = true
"#;
    let _cfg = parse(body).expect("dep capture from path is sufficient");
}

#[test]
fn rejects_invalid_line_regex() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "(^|/)rabbitmq-components\\.mk$"
when_modified_line_matches = "[unclosed"
include_suite = "x_SUITE"
"#;
    let err = parse(body).unwrap_err();
    assert!(format!("{err}").contains("invalid regex"));
}

#[test]
fn omitting_line_match_leaves_field_none() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[suite_rule]]
when_modified_path_matches = "(^|/)Makefile$"
include_suite = "x_SUITE"
"#;
    let cfg = parse(body).expect("parses without line_match");
    assert!(cfg.suite_rules[0].line_match.is_none());
    assert!(!cfg.suite_rules[0].include_suite_for_dep_modules);
}
