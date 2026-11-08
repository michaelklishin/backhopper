// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::Path;

use backhopper_core::app_src::{AppSrcSpec, parse};
use backhopper_core::model::app_manifest::{ManifestDeps, ManifestSource};
use backhopper_core::model::names::{ApplicationName, ModuleName};

fn parse_ok(text: &str) -> AppSrcSpec {
    parse(Path::new("dummy.app.src"), text).expect("parse should succeed")
}

#[test]
fn parses_minimal_app_src() {
    let text = "{application, my_app, []}.";
    let spec = parse_ok(text);
    assert_eq!(spec.name, ApplicationName::new("my_app").unwrap());
    assert!(spec.modules.is_empty());
    assert!(spec.applications.is_empty());
    assert_eq!(spec.vsn, None);
}

#[test]
fn parses_typical_app_src_with_all_known_props() {
    let text = "\
{application, rabbit_common,
 [{description, \"RabbitMQ Common Library\"},
  {vsn, \"4.2.0\"},
  {modules, [code_version, app_utils, credit_flow]},
  {registered, []},
  {applications, [kernel, stdlib, ranch]},
  {env, []}
 ]}.
";
    let spec = parse_ok(text);
    assert_eq!(spec.name, ApplicationName::new("rabbit_common").unwrap());
    assert_eq!(spec.vsn.as_deref(), Some("4.2.0"));
    assert_eq!(spec.modules.len(), 3);
    assert!(
        spec.modules
            .contains(&ModuleName::new("code_version").unwrap())
    );
    assert_eq!(spec.applications.len(), 3);
    assert!(
        spec.applications
            .contains(&ApplicationName::new("kernel").unwrap())
    );
    assert!(
        spec.applications
            .contains(&ApplicationName::new("ranch").unwrap())
    );
}

#[test]
fn ignores_unknown_props() {
    let text = "\
{application, demo,
 [{description, \"Demo\"},
  {mod, {demo_app, []}},
  {env, [{a, 1}]},
  {modules, [one, two]}
 ]}.
";
    let spec = parse_ok(text);
    assert_eq!(spec.modules.len(), 2);
    assert!(spec.modules.contains(&ModuleName::new("one").unwrap()));
    assert!(spec.modules.contains(&ModuleName::new("two").unwrap()));
}

#[test]
fn handles_line_comments() {
    let text = "\
% this is a comment
{application, demo,
 % another comment
 [{vsn, \"1.0\"}, % trailing
  {applications, [kernel, stdlib]}
 ]}.
";
    let spec = parse_ok(text);
    assert_eq!(spec.name, ApplicationName::new("demo").unwrap());
    assert_eq!(spec.vsn.as_deref(), Some("1.0"));
    assert_eq!(spec.applications.len(), 2);
}

#[test]
fn handles_empty_modules_list() {
    let text = "{application, demo, [{modules, []}, {applications, [kernel]}]}.";
    let spec = parse_ok(text);
    assert!(spec.modules.is_empty());
    assert_eq!(spec.applications.len(), 1);
}

#[test]
fn handles_quoted_atoms_in_app_name() {
    let text = "{application, 'special_app', [{modules, []}]}.";
    let spec = parse_ok(text);
    assert_eq!(spec.name.as_str(), "special_app");
}

#[test]
fn rejects_non_application_tag() {
    let result = parse(Path::new("bad.app.src"), "{module, ra_server, []}.");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("application"));
}

#[test]
fn rejects_truncated_input() {
    let result = parse(Path::new("trunc.app.src"), "{application, my_app,");
    assert!(result.is_err());
}

#[test]
fn parses_module_names_with_underscores_and_at() {
    let text = "{application, demo, [{modules, [rabbit_mgmt_wm_user, rabbit_log@db, ra_log_2]}]}.";
    let spec = parse_ok(text);
    assert_eq!(spec.modules.len(), 3);
    assert!(
        spec.modules
            .contains(&ModuleName::new("rabbit_mgmt_wm_user").unwrap())
    );
    assert!(
        spec.modules
            .contains(&ModuleName::new("rabbit_log@db").unwrap())
    );
}

#[test]
fn modules_set_dedupes_duplicates() {
    let text = "{application, demo, [{modules, [ra_log, ra_log, ra_server]}]}.";
    let spec = parse_ok(text);
    assert_eq!(spec.modules.len(), 2);
}

#[test]
fn parses_nested_tuple_in_mod_prop() {
    let text = "\
{application, demo,
 [{mod, {demo_app, [arg1, {arg2, value}]}},
  {modules, [a]}
 ]}.
";
    let spec = parse_ok(text);
    assert_eq!(spec.modules.len(), 1);
    assert!(spec.modules.contains(&ModuleName::new("a").unwrap()));
}

#[test]
fn vsn_with_embedded_quotes_is_skipped_gracefully() {
    // escapes are not interpreted: an unterminated value must still let the rest of the file parse via skip_value
    let text = "{application, demo, [{vsn, \"1.0\\\"-pre\"}, {modules, [a]}]}.";
    let spec = parse_ok(text);
    assert_eq!(spec.vsn.as_deref(), Some("1.0\\\"-pre"));
    assert!(spec.modules.contains(&ModuleName::new("a").unwrap()));
}

#[test]
fn reads_included_applications() {
    let text = "\
{application, rabbitmq_management,
 [{description, \"RabbitMQ Management Console\"},
  {applications, [kernel, stdlib, rabbit_common, rabbit]},
  {included_applications, [rabbitmq_management_agent, cowboy]}
 ]}.
";
    let spec = parse_ok(text);
    assert_eq!(spec.included_applications.len(), 2);
    assert!(
        spec.included_applications
            .contains(&ApplicationName::new("cowboy").unwrap())
    );
    assert_eq!(spec.applications.len(), 4);
}

#[test]
fn a_manifest_unions_applications_and_included_applications() {
    let text = "\
{application, rabbitmq_management,
 [{applications, [kernel, stdlib, rabbit]},
  {included_applications, [rabbit, cowboy]}
 ]}.
";
    let manifest =
        parse_ok(text).into_manifest(ApplicationName::new("rabbitmq_management").unwrap());
    assert_eq!(manifest.source, ManifestSource::AppSrc);
    let expected: BTreeSet<ApplicationName> = ["cowboy", "kernel", "rabbit", "stdlib"]
        .iter()
        .map(|n| ApplicationName::new(*n).unwrap())
        .collect();
    assert_eq!(manifest.requires, ManifestDeps::Listed(expected));
}

#[test]
fn a_non_list_included_applications_value_is_skipped() {
    let spec = parse_ok("{application, demo, [{included_applications, undefined}]}.");
    assert!(spec.included_applications.is_empty());
}
