// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::Path;

use backhopper_core::model::names::{ConfKey, ValidatorName};
use backhopper_core::model::symbol::SymbolKind;
use backhopper_cuttlefish::{
    CuttlefishFragment, FragmentKind, SchemaElements, extract_references, parse_schema,
};

const SSL_OPTIONS_PARTIAL: &str = r#"%% ssl_options.partial

{mapping, "cacertfile", "ssl_options.cacertfile",
 [{datatype, string},
  {validators, ["file_accessible"]}]}.

{mapping, "certfile", "ssl_options.certfile",
 [{datatype, string},
  {validators, ["file_accessible"]}]}.

{mapping, "verify", "ssl_options.verify",
 [{datatype, {enum, [verify_peer, verify_none]}}]}.

{mapping, "versions.$version", "ssl_options.versions",
 [{datatype, atom}]}.

{translation, "ssl_options.versions",
 fun(Conf) ->
     Settings = cuttlefish_variable:filter_by_prefix("ssl_options.versions", Conf),
     [V || {_, V} <- Settings]
 end}.

{validator, "ssl_versions", "TLS versions must be supported",
 fun(Versions) ->
     lists:all(fun ssl:is_valid_version/1, Versions)
 end}.
"#;

fn parse(source: &str) -> Vec<CuttlefishFragment> {
    parse_schema(
        source,
        Path::new("deps/rabbit/priv/schema/ssl_options.partial"),
    )
    .unwrap()
}

fn conf_keys(keys: &[&str]) -> BTreeSet<ConfKey> {
    keys.iter().map(|k| ConfKey::new(*k).unwrap()).collect()
}

fn validators(names: &[&str]) -> BTreeSet<ValidatorName> {
    names
        .iter()
        .map(|n| ValidatorName::new(*n).unwrap())
        .collect()
}

#[test]
fn a_partial_yields_its_bare_mapping_keys_and_validators() {
    let elements = SchemaElements::defined_by(&parse(SSL_OPTIONS_PARTIAL));
    assert_eq!(
        elements.mappings,
        conf_keys(&["cacertfile", "certfile", "verify", "versions.$version"])
    );
    assert_eq!(elements.validators, validators(&["ssl_versions"]));
}

#[test]
fn a_mapping_names_the_validators_it_uses() {
    let fragments = parse(SSL_OPTIONS_PARTIAL);
    let cacertfile = fragments
        .iter()
        .find(|f| f.key.as_deref() == Some("cacertfile"))
        .unwrap();
    assert_eq!(
        cacertfile.referenced_validators,
        [ValidatorName::new("file_accessible").unwrap()]
    );
    let verify = fragments
        .iter()
        .find(|f| f.key.as_deref() == Some("verify"))
        .unwrap();
    assert!(verify.referenced_validators.is_empty());
}

#[test]
fn an_inline_datatype_validator_is_a_reference() {
    let fragments = parse(
        r#"{mapping, "listeners.tcp.$name", "rabbit.tcp_listeners",
 [{datatype, [{validator, "port"}, integer]}]}."#,
    );
    assert_eq!(
        fragments[0].referenced_validators,
        [ValidatorName::new("port").unwrap()]
    );
}

#[test]
fn an_include_partial_term_yields_its_app_name_exclude_and_overrides() {
    let fragments = parse(
        r#"{include_partial, {rabbit, "ssl_options"},
 [{exclude, ["cacertfile", "versions.$version"]},
  {overrides, [{"verify", [{default, verify_peer}]}, "certfile"]}]}."#,
    );
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].kind, FragmentKind::IncludePartial);
    let include = fragments[0].partial_include.as_ref().unwrap();
    assert_eq!(include.app.as_str(), "rabbit");
    assert_eq!(include.name.as_str(), "ssl_options");
    assert_eq!(
        include.exclude,
        conf_keys(&["cacertfile", "versions.$version"])
    );
    assert_eq!(include.overrides, conf_keys(&["certfile", "verify"]));
}

#[test]
fn an_include_partial_without_options_has_no_keys() {
    let fragments = parse(r#"{include_partial, {rabbit, "tcp_listen_options"}}."#);
    let include = fragments[0].partial_include.as_ref().unwrap();
    assert_eq!(include.name.as_str(), "tcp_listen_options");
    assert!(include.exclude.is_empty());
    assert!(include.overrides.is_empty());
}

#[test]
fn an_include_partial_with_an_atom_name_is_read() {
    let fragments = parse("{include_partial, {rabbit, ssl_options}, []}.");
    let include = fragments[0].partial_include.as_ref().unwrap();
    assert_eq!(include.name.as_str(), "ssl_options");
}

#[test]
fn an_include_partial_that_names_no_app_is_not_a_partial_include() {
    let fragments = parse(r#"{include_partial, "ssl_options", []}."#);
    assert_eq!(fragments.len(), 1);
    assert!(fragments[0].partial_include.is_none());
}

#[test]
fn an_include_partial_defines_no_mapping_and_no_validator() {
    let fragments = parse(r#"{include_partial, {rabbit, "ssl_options"}, []}."#);
    assert_eq!(
        SchemaElements::defined_by(&fragments),
        SchemaElements::default()
    );
}

#[test]
fn other_fragments_carry_no_partial_include() {
    let fragments = parse(SSL_OPTIONS_PARTIAL);
    assert!(fragments.iter().all(|f| f.partial_include.is_none()));
}

#[test]
fn a_partial_translation_is_a_translation_fragment() {
    let fragments = parse(
        r#"{partial_translation, "password",
 fun(Conf) ->
     rabbit_env:read_secret(cuttlefish:conf_get("ssl_options.password", Conf))
 end}."#,
    );
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].kind, FragmentKind::Translation);
    assert_eq!(fragments[0].key.as_deref(), Some("password"));
    assert!(fragments[0].erlang_body.is_some());
}

#[test]
fn a_partial_translation_body_yields_its_calls() {
    let fragments = parse(
        r#"{partial_translation, "password",
 fun(Conf) -> rabbit_env:read_secret(Conf) end}."#,
    );
    let references = extract_references(&fragments[0]);
    assert!(
        references.iter().any(|r| matches!(
            &r.symbol.kind,
            SymbolKind::Function { mfa } if mfa.to_string() == "rabbit_env:read_secret/1"
        )),
        "{references:?}"
    );
}
