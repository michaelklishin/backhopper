// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use backhopper_core::config::{Config, ConfigFile};
use backhopper_core::model::names::{GitRef, ProjectName};
use backhopper_core::model::pin::PinSpec;

fn parse(body: &str) -> Result<Config, backhopper_core::errors::ConfigError> {
    let raw: ConfigFile = toml::from_str(body).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

const HOST_PROJECT_HEADER: &str = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name = "host"
kind = "self"
"#;

#[test]
fn parses_self_pin_with_repo_dir_path_override() {
    let body = format!(
        "{HOST_PROJECT_HEADER}\n\
[[series]]\n\
name = \"v4_1\"\n\
pins = [{{ project = \"host\", branch = \"v4.1.x\", repo_dir_path = \"/tmp/v4.1.x.git\" }}]\n"
    );
    let cfg = parse(&body).unwrap();
    let pin = &cfg.series[0].pins[0];
    let path = pin.self_repo_override().expect("override present");
    assert_eq!(path, Path::new("/tmp/v4.1.x.git"));
}

#[test]
fn self_pin_without_override_returns_none() {
    let body = format!(
        "{HOST_PROJECT_HEADER}\n\
[[series]]\n\
name = \"v4_1\"\n\
pins = [{{ project = \"host\", branch = \"v4.1.x\" }}]\n"
    );
    let cfg = parse(&body).unwrap();
    assert!(cfg.series[0].pins[0].self_repo_override().is_none());
}

#[test]
fn self_pin_sha_form_accepts_override() {
    let body = format!(
        "{HOST_PROJECT_HEADER}\n\
[[series]]\n\
name = \"locked\"\n\
pins = [{{ project = \"host\", sha = \"abcdef0123456789abcdef0123456789abcdef01\", repo_dir_path = \"/tmp/locked.git\" }}]\n"
    );
    let cfg = parse(&body).unwrap();
    let path = cfg.series[0].pins[0]
        .self_repo_override()
        .expect("override on sha-form pin");
    assert_eq!(path, Path::new("/tmp/locked.git"));
}

#[test]
fn non_self_pin_has_no_override() {
    let body = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name = "ra"
git_url = "/x"

[[series]]
name = "stable"
pins = [{ project = "ra", tag = "v2.0.0" }]
"#;
    let cfg = parse(body).unwrap();
    assert!(cfg.series[0].pins[0].self_repo_override().is_none());
}

#[test]
fn override_path_is_preserved_verbatim_through_serde() {
    let body = format!(
        "{HOST_PROJECT_HEADER}\n\
[[series]]\n\
name = \"v3_13\"\n\
pins = [{{ project = \"host\", branch = \"v3.13.x\", repo_dir_path = \"/Users/antares/Development/RabbitMQ/paid_oss_backports_v3.13.x.git\" }}]\n"
    );
    let cfg = parse(&body).unwrap();
    let path = cfg.series[0].pins[0].self_repo_override().unwrap();
    assert!(
        path.to_string_lossy()
            .contains("paid_oss_backports_v3.13.x.git")
    );
}

#[test]
fn two_self_pins_in_same_series_carry_distinct_overrides() {
    let body = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name = "host"
kind = "self"

[[series]]
name = "cascade"
pins = [
    { project = "host", branch = "v4.2.x", repo_dir_path = "/tmp/v4.2.x.git" },
    { project = "host", branch = "v4.1.x", repo_dir_path = "/tmp/v4.1.x.git" },
]
"#;
    let cfg = parse(body).unwrap();
    let p0 = cfg.series[0].pins[0].self_repo_override().unwrap();
    let p1 = cfg.series[0].pins[1].self_repo_override().unwrap();
    assert_ne!(p0, p1);
    assert!(p0.to_string_lossy().contains("v4.2.x.git"));
    assert!(p1.to_string_lossy().contains("v4.1.x.git"));
}

#[test]
fn self_repo_override_on_pinspec_constructed_in_code() {
    let spec = PinSpec::SelfRef {
        project: ProjectName::new("host").unwrap(),
        git_ref: GitRef::new("v4.1.x").unwrap(),
        repo_dir_path: Some(PathBuf::from("/tmp/branch.git")),
    };
    assert_eq!(
        spec.self_repo_override(),
        Some(Path::new("/tmp/branch.git"))
    );
}
