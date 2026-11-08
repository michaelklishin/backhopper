// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::config::{Config, ConfigFile, PinSource};
use backhopper_core::errors::ConfigError;
use backhopper_core::model::names::HostName;
use backhopper_core::model::release_roots::ReleaseRoots;
use backhopper_core::model::release_version::HostVersion;

use crate::jigsaw_support::{CONFIG, config, rabbitmq};

fn load(text: &str) -> Result<Config, ConfigError> {
    let raw: ConfigFile = toml::from_str(text).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

fn with_replacement(from: &str, to: &str) -> String {
    assert!(CONFIG.contains(from), "{from:?} is not in the fixture");
    CONFIG.replacen(from, to, 1)
}

#[test]
fn the_documented_host_loads() {
    let cfg = config();
    let host = rabbitmq(&cfg);
    assert_eq!(host.name.as_str(), "rabbitmq");
    assert_eq!(host.project.as_str(), "rabbitmq-server");
    assert_eq!(host.otp_project.as_str(), "otp");
    assert_eq!(host.series_tag_pattern.as_str(), "v{series}.*");
    let majors: Vec<u16> = host.erlang_majors.iter().map(|m| m.get()).collect();
    assert_eq!(majors, vec![27, 28]);
    assert!(
        matches!(&host.pins, PinSource::ErlangMk { file_path } if file_path.as_str() == "rabbitmq-components.mk")
    );
    assert!(matches!(&host.release_roots, ReleaseRoots::Both { .. }));
    let host_name = HostName::new("rabbitmq").unwrap();
    assert_eq!(cfg.host(&host_name).unwrap().name, host_name);
}

#[test]
fn an_unknown_host_is_an_error() {
    let missing = HostName::new("zeromq").unwrap();
    assert!(matches!(
        config().host(&missing),
        Err(ConfigError::UnknownHost(name)) if name == "zeromq"
    ));
}

#[test]
fn erlang_support_and_its_file_together_are_refused() {
    let text = with_replacement(
        "[host.pins]",
        "erlang_support_file_path = \"support.toml\"\n\n[host.pins]",
    );
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("mutually exclusive"), "{error}");
}

#[test]
fn neither_erlang_support_door_is_refused() {
    let text = CONFIG.split("[host.erlang_support]").next().unwrap();
    let error = load(text).unwrap_err();
    assert!(
        error.to_string().contains("one of erlang_support"),
        "{error}"
    );
}

#[test]
fn erlang_support_is_read_from_a_file_next_to_the_config() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("support.toml"),
        "default = \"[27.0,30.0)\"\n[releases]\n\"4.2.9\" = \"[27.0,29.0)\"\n",
    )
    .unwrap();
    let text = CONFIG
        .split("[host.erlang_support]")
        .next()
        .unwrap()
        .replacen(
            "[host.pins]",
            "erlang_support_file_path = \"support.toml\"\n\n[host.pins]",
            1,
        );
    let raw: ConfigFile = toml::from_str(&text).unwrap();
    let cfg = Config::from_raw(dir.path().join("backhopper.toml"), raw).unwrap();
    let range = cfg.hosts[0]
        .erlang_support
        .range_for(&"4.2.9".parse::<HostVersion>().unwrap());
    assert_eq!(range.to_string(), "[27.0,29.0)");
}

#[test]
fn a_missing_erlang_support_file_is_refused_with_its_path() {
    let dir = TempDir::new().unwrap();
    let text = CONFIG
        .split("[host.erlang_support]")
        .next()
        .unwrap()
        .replacen(
            "[host.pins]",
            "erlang_support_file_path = \"absent.toml\"\n\n[host.pins]",
            1,
        );
    let raw: ConfigFile = toml::from_str(&text).unwrap();
    let error = Config::from_raw(dir.path().join("backhopper.toml"), raw).unwrap_err();
    assert!(error.to_string().contains("absent.toml"), "{error}");
}

#[test]
fn a_range_with_a_closed_top_is_refused_naming_the_release() {
    let text = with_replacement("\"4.2.9\" = \"[27.0,29.0)\"", "\"4.2.9\" = \"[27.0,29.0]\"");
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("4.2.9"), "{error}");
}

#[test]
fn a_bad_default_range_is_refused() {
    let text = with_replacement("default = \"[27.0,30.0)\"", "default = \"27.0..30.0\"");
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("default"), "{error}");
}

#[test]
fn a_series_tag_pattern_without_the_placeholder_is_refused() {
    let text = with_replacement(
        "series_tag_pattern = \"v{series}.*\"",
        "series_tag_pattern = \"v*\"",
    );
    assert!(load(&text).is_err());
}

#[test]
fn a_host_with_no_erlang_majors_is_refused() {
    let text = with_replacement("erlang_majors = [27, 28]", "erlang_majors = []");
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("at least one major"), "{error}");
}

#[test]
fn erlang_majors_are_sorted_and_deduplicated() {
    let text = with_replacement("erlang_majors = [27, 28]", "erlang_majors = [28, 27, 28]");
    let cfg = load(&text).unwrap();
    let majors: Vec<u16> = cfg.hosts[0].erlang_majors.iter().map(|m| m.get()).collect();
    assert_eq!(majors, vec![27, 28]);
}

#[test]
fn a_host_whose_project_is_the_self_project_is_refused() {
    let text = with_replacement(
        "git_url = \"/srv/checkouts/rabbitmq-server.git\"",
        "kind = \"self\"",
    );
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("self project"), "{error}");
}

#[test]
fn a_host_naming_an_unconfigured_project_is_refused() {
    let text = with_replacement(
        "project = \"rabbitmq-server\"",
        "project = \"rabbitmq-broker\"",
    );
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("rabbitmq-broker"), "{error}");
}

#[test]
fn an_otp_project_must_use_the_erlang_otp_layout() {
    let text = with_replacement("otp_project = \"otp\"", "otp_project = \"ra\"");
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("erlang_otp"), "{error}");
}

#[test]
fn an_otp_project_with_another_tag_prefix_is_refused() {
    let text = with_replacement(
        "layout = \"erlang_otp\"",
        "layout = \"erlang_otp\"\ntag_prefix = \"v\"",
    );
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("OTP-"), "{error}");
}

#[test]
fn an_otp_project_may_state_the_default_tag_prefix() {
    let text = with_replacement(
        "layout = \"erlang_otp\"",
        "layout = \"erlang_otp\"\ntag_prefix = \"OTP-\"",
    );
    assert!(load(&text).is_ok());
}

#[test]
fn an_unknown_key_under_pins_is_refused() {
    let text = with_replacement("[host.pins]\n", "[host.pins]\nbranch = \"main\"\n");
    assert!(toml::from_str::<ConfigFile>(&text).is_err());
}

#[test]
fn the_table_pin_source_loads() {
    let text = with_replacement(
        "source = \"erlang_mk\"\nfile_path = \"rabbitmq-components.mk\"",
        "source = \"table\"\n\n[host.pins.releases.\"4.2.0\"]\nra = \"2.17.1\"\nkhepri = \"0.17.2\"",
    );
    let cfg = load(&text).unwrap();
    let PinSource::Table(table) = &cfg.hosts[0].pins else {
        panic!("expected a table");
    };
    let release = table.get(&"4.2.0".parse::<HostVersion>().unwrap()).unwrap();
    assert_eq!(release.len(), 2);
}

#[test]
fn pin_source_fields_must_match_the_source() {
    let table_with_file = with_replacement("source = \"erlang_mk\"", "source = \"table\"");
    assert!(load(&table_with_file).is_err());
    let file_source_without_file = with_replacement(
        "source = \"erlang_mk\"\nfile_path = \"rabbitmq-components.mk\"",
        "source = \"erlang_mk\"",
    );
    assert!(load(&file_source_without_file).is_err());
    let unknown = with_replacement("source = \"erlang_mk\"", "source = \"hex\"");
    assert!(load(&unknown).is_err());
}

#[test]
fn release_roots_need_apps_or_a_variable() {
    let neither = with_replacement(
        "apps = [\"rabbit\"]\nmakefile_variable = { file_path = \"plugins.mk\", variable = \"PLUGINS\" }",
        "",
    );
    assert!(load(&neither).is_err());
    let empty_apps = with_replacement("apps = [\"rabbit\"]", "apps = []");
    assert!(load(&empty_apps).is_err());
    let apps_only = with_replacement(
        "makefile_variable = { file_path = \"plugins.mk\", variable = \"PLUGINS\" }",
        "",
    );
    assert!(matches!(
        load(&apps_only).unwrap().hosts[0].release_roots,
        ReleaseRoots::Apps(_)
    ));
}

#[test]
fn a_host_declared_twice_is_refused() {
    let host_block = CONFIG.split("[[host]]").nth(1).unwrap();
    let text = format!("{CONFIG}\n[[host]]{host_block}");
    let error = load(&text).unwrap_err();
    assert!(error.to_string().contains("duplicate"), "{error}");
}

#[test]
fn the_resolved_config_serializes_with_its_hosts() {
    let value = serde_json::to_value(config()).unwrap();
    assert_eq!(value["hosts"][0]["name"], "rabbitmq");
    assert_eq!(
        value["hosts"][0]["erlang_support"]["default"],
        "[27.0,30.0)"
    );
}
