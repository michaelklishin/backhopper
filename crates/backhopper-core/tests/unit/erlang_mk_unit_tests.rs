// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};

use backhopper_core::erlang_mk::{
    DepPin, DepSource, ErlangMkManifest, MakeVariables, dep_to_tag, parse_components_mk,
    parse_pin_line, scan_components_mk, version_to_tag,
};
use backhopper_core::model::app_manifest::{ManifestDeps, ManifestSource, ManifestUnreadable};
use backhopper_core::model::names::{ApplicationName, DependencyName, MakeVariableName};

const AMQP_CLIENT_MAKEFILE: &str = "\
PROJECT = rabbitmq_amqp_client
PROJECT_DESCRIPTION = AMQP 1.0 client for RabbitMQ

define PROJECT_APP_EXTRA_KEYS
\t{broker_version_requirements, []}
endef

BUILD_DEPS = rabbit_common
DEPS = amqp10_client rabbit_common
LOCAL_DEPS = ssl inets
TEST_DEPS = rabbitmq_ct_helpers rabbitmq_ct_client_helpers
DEP_PLUGINS = rabbit_common/mk/rabbitmq-build.mk

DEP_EARLY_PLUGINS = rabbit_common/mk/rabbitmq-early-plugin.mk

include ../../rabbitmq-components.mk
include ../../erlang.mk
";

fn apps(names: &[&str]) -> ManifestDeps {
    ManifestDeps::Listed(
        names
            .iter()
            .map(|n| ApplicationName::new(*n).unwrap())
            .collect::<BTreeSet<_>>(),
    )
}

#[test]
fn reads_project_deps_and_local_deps_from_a_realistic_makefile() {
    let manifest = ErlangMkManifest::parse(AMQP_CLIENT_MAKEFILE);
    assert_eq!(
        manifest.project,
        Some(ApplicationName::new("rabbitmq_amqp_client").unwrap())
    );
    assert_eq!(
        manifest.requires,
        apps(&["amqp10_client", "inets", "rabbit_common", "ssl"])
    );
}

#[test]
fn test_deps_and_build_deps_do_not_appear() {
    let ManifestDeps::Listed(requires) = ErlangMkManifest::parse(AMQP_CLIENT_MAKEFILE).requires
    else {
        panic!("expected a readable list");
    };
    for left_out in ["rabbitmq_ct_helpers", "rabbitmq_ct_client_helpers"] {
        assert!(!requires.contains(&ApplicationName::new(left_out).unwrap()));
    }
    let build_only = ErlangMkManifest::parse("BUILD_DEPS = rabbitmq_cli\nDEPS = ranch\n");
    assert_eq!(build_only.requires, apps(&["ranch"]));
}

#[test]
fn plus_equals_appends() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch\nDEPS += cowlib\nLOCAL_DEPS += ssl\n");
    assert_eq!(manifest.requires, apps(&["cowlib", "ranch", "ssl"]));
}

#[test]
fn a_backslash_continuation_joins_lines() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch \\\n  cowlib \\\n  cowboy\n");
    assert_eq!(manifest.requires, apps(&["cowboy", "cowlib", "ranch"]));
}

#[test]
fn a_variable_reference_makes_the_whole_list_unexpanded() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch $(EXTRA)\nLOCAL_DEPS = ssl\n");
    assert_eq!(
        manifest.requires,
        ManifestDeps::Unreadable(ManifestUnreadable::UnexpandedVariable(
            MakeVariableName::new("EXTRA").unwrap()
        ))
    );
}

#[test]
fn a_brace_reference_is_unexpanded_too() {
    let manifest = ErlangMkManifest::parse("LOCAL_DEPS = ${PLUGINS}\n");
    assert_eq!(
        manifest.requires,
        ManifestDeps::Unreadable(ManifestUnreadable::UnexpandedVariable(
            MakeVariableName::new("PLUGINS").unwrap()
        ))
    );
}

#[test]
fn a_word_that_is_not_an_app_name_fails_the_parse() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch Rabbit-Common\n");
    assert_eq!(
        manifest.requires,
        ManifestDeps::Unreadable(ManifestUnreadable::ParseFailed)
    );
}

#[test]
fn a_makefile_with_no_deps_lists_none() {
    let manifest = ErlangMkManifest::parse("PROJECT = cowboy\n");
    assert_eq!(manifest.requires, apps(&[]));
}

#[test]
fn question_equals_keeps_an_earlier_value() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch\nDEPS ?= cowlib\nLOCAL_DEPS ?= ssl\n");
    assert_eq!(manifest.requires, apps(&["ranch", "ssl"]));
}

#[test]
fn simple_and_posix_assignment_replace() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch\nDEPS := cowlib\nLOCAL_DEPS ::= ssl\n");
    assert_eq!(manifest.requires, apps(&["cowlib", "ssl"]));
}

#[test]
fn comments_and_recipes_and_define_blocks_are_ignored() {
    let text = "\
# DEPS = nothing
DEPS = ranch # cowlib
all:
\tDEPS = recipe_line
define SNIPPET
DEPS = inside_define
endef
";
    assert_eq!(ErlangMkManifest::parse(text).requires, apps(&["ranch"]));
}

#[test]
fn include_lines_and_conditionals_are_not_evaluated() {
    let text = "include ../../rabbitmq-components.mk\nifeq ($(X),1)\nDEPS = ranch\nendif\n";
    assert_eq!(ErlangMkManifest::parse(text).requires, apps(&["ranch"]));
}

#[test]
fn project_that_is_not_an_app_name_is_none() {
    assert_eq!(
        ErlangMkManifest::parse("PROJECT = Rabbit-MQ\n").project,
        None
    );
}

#[test]
fn manifest_names_the_erlang_mk_source() {
    let manifest = ErlangMkManifest::parse("DEPS = ranch\n")
        .into_manifest(ApplicationName::new("rabbit").unwrap());
    assert_eq!(manifest.source, ManifestSource::ErlangMk);
    assert_eq!(manifest.name.as_str(), "rabbit");
}

#[test]
fn words_of_a_named_variable_come_from_its_file() {
    let variables =
        MakeVariables::parse("PLUGINS := rabbitmq_management rabbitmq_mqtt \\\n  rabbitmq_stomp\n");
    assert_eq!(
        variables.words("PLUGINS").unwrap(),
        ["rabbitmq_management", "rabbitmq_mqtt", "rabbitmq_stomp"]
    );
}

#[test]
fn an_unassigned_variable_is_missing() {
    let variables = MakeVariables::parse("DEPS = ranch\n");
    assert_eq!(variables.words("PLUGINS"), Err(ManifestUnreadable::Missing));
}

const COMPONENTS_SAMPLE: &str = r#"
# RabbitMQ-style dep pins
dep_cowboy = hex 2.14.1
dep_khepri = hex 0.18.0
dep_osiris = git https://github.com/rabbitmq/osiris v1.13.1
dep_ra = hex 3.1.6
dep_jose = hex 1.11.12
dep_some_alias = hex 1.2.3 actual_pkg_name
dep_aux = git_rmq cowboy 2.13.0

# community plugins use a function call we should ignore
dep_rabbitmq_lvc_exchange = $(call community_dep,rabbitmq-lvc-exchange)

# unrelated lines
PROJECT = rabbit
some_other_var = no
"#;

fn dependency(name: &str) -> DependencyName {
    DependencyName::new(name).unwrap()
}

#[test]
fn parses_canonical_hex_and_git_lines() {
    let pins = parse_components_mk(COMPONENTS_SAMPLE);
    let by_name: BTreeMap<_, _> = pins.iter().map(|p| (p.name.as_str(), p)).collect();
    assert_eq!(by_name["cowboy"].source, DepSource::Hex);
    assert_eq!(by_name["cowboy"].version, "2.14.1");
    assert_eq!(by_name["khepri"].version, "0.18.0");
    assert_eq!(by_name["osiris"].source, DepSource::Git);
    assert_eq!(by_name["osiris"].version, "v1.13.1");
    assert_eq!(by_name["aux"].source, DepSource::GitRmq);
    assert_eq!(by_name["aux"].version, "2.13.0");
}

#[test]
fn ignores_function_call_form_and_unrelated_lines() {
    let pins = parse_components_mk(COMPONENTS_SAMPLE);
    assert!(!pins.iter().any(|p| p.name == "rabbitmq_lvc_exchange"));
    assert!(!pins.iter().any(|p| p.name == "PROJECT"));
}

#[test]
fn keeps_only_first_word_when_hex_has_alternate_pkg() {
    let pins = parse_components_mk("dep_x = hex 1.2.3 alt_name\n");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].version, "1.2.3");
}

#[test]
fn ignores_comment_lines() {
    let pins = parse_components_mk("# dep_foo = hex 1.0.0\n   # dep_bar = hex 2.0.0\n");
    assert!(pins.is_empty());
}

#[test]
fn skips_dep_commit_overrides() {
    assert!(parse_components_mk("dep_ra_commit = abcdef1234\n").is_empty());
}

#[test]
fn drops_a_name_the_name_type_refuses() {
    assert!(parse_components_mk("dep_9lives = hex 1.0.0\n").is_empty());
}

#[test]
fn parse_pin_line_accepts_each_source_form() {
    assert_eq!(
        parse_pin_line("dep_ra = hex 3.1.6").unwrap().source,
        DepSource::Hex
    );
    assert_eq!(
        parse_pin_line("dep_osiris = git https://github.com/rabbitmq/osiris v1.13.1")
            .unwrap()
            .version,
        "v1.13.1"
    );
    assert_eq!(
        parse_pin_line("dep_cowboy = git_rmq cowboy 2.13.0")
            .unwrap()
            .version,
        "2.13.0"
    );
}

#[test]
fn parse_pin_line_rejects_comments_and_noise() {
    assert_eq!(parse_pin_line("# dep_ra = hex 3.1.6"), None);
    assert_eq!(parse_pin_line("PROJECT = rabbit"), None);
    assert_eq!(parse_pin_line("dep_ra_commit = abcdef"), None);
    assert_eq!(
        parse_pin_line("dep_lvc = $(call community_dep,rabbitmq-lvc-exchange)"),
        None
    );
}

#[test]
fn display_and_parse_display_round_trip() {
    for pin in parse_components_mk(COMPONENTS_SAMPLE) {
        let displayed = pin.display();
        let back = DepPin::parse_display(&pin.name, &displayed).unwrap();
        assert_eq!(back, pin);
    }
}

#[test]
fn parse_display_rejects_unknown_source_labels() {
    assert_eq!(
        DepPin::parse_display(&dependency("ra"), "cargo 1.0.0"),
        None
    );
    assert_eq!(DepPin::parse_display(&dependency("ra"), "3.1.6"), None);
}

#[test]
fn scanning_reports_the_names_the_name_type_refuses() {
    let scan = scan_components_mk(
        "dep_ra = hex 3.1.6\ndep_3lib = hex 1.0.0\ndep_9lives = git https://example.com/9.git v1\n",
    );
    assert_eq!(scan.pins.len(), 1);
    assert_eq!(scan.refused_names, ["3lib", "9lives"]);
}

#[test]
fn a_hex_dep_takes_the_project_tag_prefix_and_a_git_dep_is_verbatim() {
    let hex = parse_pin_line("dep_ra = hex 2.16.7").unwrap();
    assert_eq!(dep_to_tag(&hex, "v"), "v2.16.7");
    assert_eq!(dep_to_tag(&hex, ""), "2.16.7");
    let git =
        parse_pin_line("dep_osiris = git https://github.com/rabbitmq/osiris v1.10.3").unwrap();
    assert_eq!(dep_to_tag(&git, "v"), "v1.10.3");
    let forked = parse_pin_line("dep_cowboy = git_rmq cowboy 2.13.0").unwrap();
    assert_eq!(dep_to_tag(&forked, "v"), "2.13.0");
}

#[test]
fn a_version_keeps_a_tag_prefix_it_already_has() {
    assert_eq!(version_to_tag("3.1.6", "v"), "v3.1.6");
    assert_eq!(version_to_tag("v3.1.6", "v"), "v3.1.6");
    assert_eq!(version_to_tag("2.14.1", ""), "2.14.1");
}
