// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use backhopper_core::config::{Config, ConfigFile, ProjectKind, ProjectSource};
use backhopper_core::model::pin::PinSpec;

fn parse(body: &str) -> Result<Config, backhopper_core::errors::ConfigError> {
    let raw: ConfigFile = toml::from_str(body).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

#[test]
fn an_external_project_needs_a_git_url() {
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
    assert!(matches!(
        cfg.projects[0].source,
        ProjectSource::External { .. }
    ));
}

#[test]
fn a_self_project_cannot_carry_a_git_url() {
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
    assert_eq!(cfg.projects[0].source, ProjectSource::SelfRepo);
}

#[test]
fn config_show_json_spells_a_self_project_self() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "host"
kind = "self"
"#;
    let cfg = parse(body).unwrap();
    let json = serde_json::to_value(&cfg.projects[0]).unwrap();
    assert_eq!(json.get("kind").and_then(|v| v.as_str()), Some("self"));
}

#[test]
fn self_project_is_none_when_every_project_is_external() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name    = "ra"
git_url = "/tmp/ra.git"
"#;
    let cfg = parse(body).unwrap();
    assert!(cfg.self_project().is_none());
}

#[test]
fn self_project_names_the_one_self_project() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"
[[project]]
name = "host"
kind = "self"
[[project]]
name    = "ra"
git_url = "/tmp/ra.git"
"#;
    let cfg = parse(body).unwrap();
    assert_eq!(cfg.self_project().unwrap().name.as_str(), "host");
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
        PinSpec::SelfRef {
            project, git_ref, ..
        } => {
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

#[test]
fn project_source_projects_back_to_its_raw_kind() {
    let external = ProjectSource::External {
        git_url: PathBuf::from("/tmp/ra.git"),
    };
    assert_eq!(external.kind(), ProjectKind::External);
    assert_eq!(ProjectSource::SelfRepo.kind(), ProjectKind::SelfRepo);
    assert_eq!(ProjectSource::SelfRepo.kind().label(), "self");
}

#[test]
fn only_an_external_project_source_has_a_git_url() {
    let external = ProjectSource::External {
        git_url: PathBuf::from("/tmp/ra.git"),
    };
    assert_eq!(external.git_url(), Some(Path::new("/tmp/ra.git")));
    assert_eq!(ProjectSource::SelfRepo.git_url(), None);
}
