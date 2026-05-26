// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::config::{Config, ConfigFile, ProjectKind};
use backhopper_core::model::pin::PinSpec;

fn parse(body: &str) -> Result<Config, backhopper_core::errors::ConfigError> {
    let raw: ConfigFile = toml::from_str(body).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

#[test]
fn project_defaults_to_external_kind_and_requires_git_url() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name    = "ra"
git_url = "/tmp/ra.git"
"#;
    let cfg = parse(body).unwrap();
    assert_eq!(cfg.projects[0].kind, ProjectKind::External);
    assert!(cfg.projects[0].git_url.is_some());
}

#[test]
fn external_project_without_git_url_is_rejected() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "ra"
"#;
    let err = parse(body).unwrap_err();
    assert!(format!("{err}").contains("requires git_url"), "got: {err}");
}

#[test]
fn self_project_must_not_set_git_url() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name    = "host"
kind    = "self"
git_url = "/tmp/host.git"
"#;
    let err = parse(body).unwrap_err();
    assert!(format!("{err}").contains("kind=\"self\""), "got: {err}");
}

#[test]
fn self_project_parses_without_git_url() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "host"
kind = "self"
"#;
    let cfg = parse(body).unwrap();
    assert_eq!(cfg.projects[0].kind, ProjectKind::SelfRepo);
    assert!(cfg.projects[0].git_url.is_none());
}

#[test]
fn more_than_one_self_project_is_rejected() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "host_a"
kind = "self"
[[project]]
name = "host_b"
kind = "self"
"#;
    let err = parse(body).unwrap_err();
    assert!(
        format!("{err}").contains("more than one project"),
        "got: {err}"
    );
}

#[test]
fn unknown_kind_is_rejected() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "x"
kind = "weird"
"#;
    let err = parse(body).unwrap_err();
    assert!(
        format!("{err}").contains("unknown project kind"),
        "got: {err}"
    );
}

#[test]
fn self_ref_pin_parses_with_branch() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "host"
kind = "self"
[[series]]
name = "host-branch"
pins = [{ project = "host", branch = "v4.2.x" }]
"#;
    let cfg = parse(body).unwrap();
    match &cfg.series[0].pins[0] {
        PinSpec::SelfRef { project, git_ref } => {
            assert_eq!(project.as_str(), "host");
            assert_eq!(git_ref.as_str(), "v4.2.x");
        }
        other => panic!("expected SelfRef, got {other:?}"),
    }
}

#[test]
fn self_pin_referencing_external_project_is_rejected() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name    = "ra"
git_url = "/tmp/ra.git"
[[series]]
name = "rabbitmq-4.2"
pins = [{ project = "ra", branch = "v4.2.x" }]
"#;
    let err = parse(body).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("self-pin") && msg.contains("ra"),
        "expected self-pin-on-external-project error, got: {msg}"
    );
}

#[test]
fn self_ref_pin_parses_with_sha() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "host"
kind = "self"
[[series]]
name = "host-pinned"
pins = [{ project = "host", sha = "deadbeef" }]
"#;
    let cfg = parse(body).unwrap();
    match &cfg.series[0].pins[0] {
        PinSpec::SelfRef { git_ref, .. } => {
            assert_eq!(git_ref.as_str(), "deadbeef");
        }
        other => panic!("expected SelfRef, got {other:?}"),
    }
}
