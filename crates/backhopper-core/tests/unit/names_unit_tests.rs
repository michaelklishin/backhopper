// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::errors::NameError;
use backhopper_core::model::names::{
    ApplicationName, Arity, BehaviourName, CommitSha, FunctionName, Mfa, ModuleName, ProjectName,
    SeriesName, TagName,
};

#[test]
fn project_name_accepts_lowercase_alnum_dash_underscore() {
    ProjectName::new("ra").expect("valid");
    ProjectName::new("rabbitmq-http-api-client-rs").expect("valid");
    ProjectName::new("frm_2").expect("valid");
}

#[test]
fn project_name_rejects_uppercase_starts_or_paths() {
    assert!(ProjectName::new("Ra").is_err());
    assert!(ProjectName::new("../escape").is_err());
    assert!(ProjectName::new("").is_err());
    assert!(ProjectName::new("9starts-digit").is_err());
}

#[test]
fn series_name_allows_dots_for_release_lines() {
    SeriesName::new("rabbitmq-4.2").expect("valid");
    SeriesName::new("rabbitmq-3.13").expect("valid");
    assert!(SeriesName::new("CAPS").is_err());
}

#[test]
fn tag_name_rejects_path_separators() {
    assert!(TagName::new("v3.1.6").is_ok());
    assert!(TagName::new("2.14.1").is_ok());
    assert!(TagName::new("v3/1/6").is_err());
    assert!(TagName::new("..").is_err());
    assert!(TagName::new(".hidden").is_err());
    assert!(TagName::new("with space").is_err());
}

#[test]
fn tag_name_rejects_git_ref_magic() {
    assert!(TagName::new("HEAD~1").is_err());
    assert!(TagName::new("v1^2").is_err());
    assert!(TagName::new("v1:main").is_err());
}

#[test]
fn module_name_accepts_erlang_and_elixir_forms() {
    ModuleName::new("ra").expect("valid Erlang atom");
    ModuleName::new("rabbit_amqp_reader").expect("valid Erlang atom");
    ModuleName::new("'Quoted Atom'").expect("valid quoted");
    ModuleName::new("Plug").expect("valid Elixir module");
    ModuleName::new("Plug.Conn").expect("valid Elixir nested module");
    ModuleName::new("RabbitMQ.CLI.Ctl").expect("valid Elixir deep module");
    assert!(ModuleName::new("").is_err());
    assert!(ModuleName::new("9starts_digit").is_err());
    assert!(ModuleName::new("has space").is_err());
}

#[test]
fn arity_round_trip_via_from_str() {
    for v in [0u8, 1, 2, 10, 255] {
        let parsed = Arity::from_str(&v.to_string()).unwrap();
        assert_eq!(parsed.get(), v);
    }
    assert!(Arity::from_str("256").is_err());
    assert!(Arity::from_str("-1").is_err());
    assert!(Arity::from_str("abc").is_err());
}

#[test]
fn arity_out_of_range_reports_actual_value() {
    match Arity::from_str("256") {
        Err(NameError::InvalidArity { value }) => assert_eq!(value, 256),
        other => panic!("expected InvalidArity(256), got {other:?}"),
    }
    match Arity::from_str("-1") {
        Err(NameError::InvalidArity { value }) => assert_eq!(value, -1),
        other => panic!("expected InvalidArity(-1), got {other:?}"),
    }
}

#[test]
fn arity_non_numeric_reports_raw_input() {
    match Arity::from_str("abc") {
        Err(NameError::InvalidArityParse { raw }) => assert_eq!(raw, "abc"),
        other => panic!("expected InvalidArityParse(\"abc\"), got {other:?}"),
    }
    match Arity::from_str("") {
        Err(NameError::InvalidArityParse { raw }) => assert_eq!(raw, ""),
        other => panic!("expected InvalidArityParse(\"\"), got {other:?}"),
    }
}

#[test]
fn mfa_round_trip_via_from_str() {
    let s = "cowboy_req:set_resp_header/3";
    let parsed: Mfa = s.parse().unwrap();
    assert_eq!(parsed.module.as_str(), "cowboy_req");
    assert_eq!(parsed.function.as_str(), "set_resp_header");
    assert_eq!(parsed.arity.get(), 3);
    assert_eq!(parsed.to_string(), s);
}

#[test]
fn mfa_rejects_garbage() {
    assert!(Mfa::from_str("noslash").is_err());
    assert!(Mfa::from_str("no:colon").is_err());
    assert!(Mfa::from_str("ra:members/").is_err());
    assert!(Mfa::from_str(":members/1").is_err());
}

#[test]
fn commit_sha_validates_40_hex_lowercase() {
    let sha = "0".repeat(40);
    assert!(CommitSha::new(sha).is_ok());
    assert!(CommitSha::new("0".repeat(39)).is_err());
    assert!(CommitSha::new("X".repeat(40)).is_err());
    assert!(CommitSha::new("ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD").is_err());
}

#[test]
fn function_name_round_trip() {
    let f = FunctionName::from_str("process_command@1").unwrap();
    assert_eq!(f.to_string(), "process_command@1");
}

#[test]
fn application_name_accepts_lowercase_underscore() {
    assert!(ApplicationName::new("rabbit".to_owned()).is_ok());
    assert!(ApplicationName::new("rabbitmq_management".to_owned()).is_ok());
    assert!(ApplicationName::new("kernel".to_owned()).is_ok());
}

#[test]
fn application_name_rejects_uppercase_and_dashes() {
    assert!(ApplicationName::new("Rabbit".to_owned()).is_err());
    assert!(ApplicationName::new("rabbit-mq".to_owned()).is_err());
    assert!(ApplicationName::new(String::new()).is_err());
}

#[test]
fn behaviour_name_round_trips() {
    let b = BehaviourName::from_str("gen_server").unwrap();
    assert_eq!(b.to_string(), "gen_server");
}

#[test]
fn mfa_implements_ord_for_btree_keys() {
    use std::collections::BTreeMap;
    let m1: Mfa = "ra:members/0".parse().unwrap();
    let m2: Mfa = "ra:overview/0".parse().unwrap();
    let mut map = BTreeMap::new();
    map.insert(m1.clone(), 1);
    map.insert(m2.clone(), 2);
    let keys: Vec<&Mfa> = map.keys().collect();
    assert_eq!(keys, vec![&m1, &m2]);
}
