// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use backhopper_core::errors::SnapshotError;
use backhopper_core::model::app_manifest::{
    AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable,
};
use backhopper_core::model::macro_decl::{MacroBody, MacroDecl, MacroKey};
use backhopper_core::model::names::{
    ApplicationName, Arity, ConfKey, DependencyName, DependencyVersion, ExtractionSettingsDigest,
    FieldName, HostFactsDigest, MacroName, MakeVariableName, ModuleName, PartialName, RecordName,
    RelativePath, ValidatorName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::normalised::{NormalisedDefault, NormalisedMacroBody};
use backhopper_core::model::schema_files::{PartialFile, SchemaFile};
use backhopper_core::model::snapshot::{
    HeaderRef, HostReleaseFacts, HrlFile, Module, RecordDecl, RecordField, Snapshot,
    SnapshotHeader, VendoredDep, VendoredDepSource, state,
};
use backhopper_core::snapshot::{format, parser};
use backhopper_test_support::snapshot_header;

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn path(p: &str) -> RelativePath {
    RelativePath::new(p).unwrap()
}

fn digest() -> HostFactsDigest {
    HostFactsDigest::new("a".repeat(64)).unwrap()
}

fn v5_header() -> SnapshotHeader {
    let mut header = snapshot_header("rabbitmq-server", "v4.2.8");
    header.format_version = 5;
    header
}

fn manifest(name: &str, requires: &[&str]) -> AppManifest {
    AppManifest {
        name: app(name),
        source: ManifestSource::ErlangMk,
        requires: ManifestDeps::Listed(requires.iter().map(|r| app(r)).collect()),
    }
}

fn module(name: &str, in_app: &str, file: &str) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.app = Some(app(in_app));
    m.path = Some(path(file));
    m
}

fn text_define(name: &str, arity: Option<u8>, body: &str) -> MacroDecl {
    MacroDecl {
        key: MacroKey {
            name: MacroName::new(name).unwrap(),
            arity: arity.map(Arity::new),
        },
        body: MacroBody::Text(NormalisedMacroBody::from_source(body)),
    }
}

fn field(name: &str, type_repr: Option<&str>, default: Option<&str>) -> RecordField {
    RecordField {
        name: FieldName::new(name).unwrap(),
        type_repr: type_repr.map(str::to_owned),
        default: default.and_then(NormalisedDefault::from_source),
    }
}

fn rabbit_header_file() -> HrlFile {
    let mut hrl = HrlFile::new(path("deps/rabbit_common/include/rabbit.hrl"));
    hrl.app = Some(app("rabbit_common"));
    hrl.includes = vec![HeaderRef {
        app: app("rabbit_common"),
        app_path: path("include/resource.hrl"),
    }];
    hrl.records = vec![RecordDecl {
        name: RecordName::new("amqqueue").unwrap(),
        fields: vec![
            field("name", Some("rabbit_types:r(queue)"), None),
            field("durable", Some("boolean()"), Some("false")),
            field("arguments", None, Some("[]")),
        ],
    }];
    hrl.defines = vec![
        text_define("DEFAULT_CONSUMER_TIMEOUT", None, "1800000"),
        text_define("IS_QUEUE", Some(1), "is_record(Q, amqqueue)"),
        MacroDecl {
            key: MacroKey::object_like(MacroName::new("ROUTING_KEY_LIMIT").unwrap()),
            body: MacroBody::Conditional,
        },
    ];
    hrl
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

fn full_snapshot() -> Snapshot<state::Canonical> {
    let mut header = v5_header();
    header.dep_pins = vec![VendoredDep {
        name: DependencyName::new("ra").unwrap(),
        version: DependencyVersion::new("2.17.1").unwrap(),
        source: VendoredDepSource::Hex,
    }];
    header.host_facts = Some(HostReleaseFacts {
        release_roots: NonEmpty::new(app("rabbit"), [app("rabbitmq_management")]),
        digest: digest(),
    });
    Snapshot::from_extracted(
        header,
        vec![
            module(
                "rabbit_amqqueue",
                "rabbit",
                "deps/rabbit/src/rabbit_amqqueue.erl",
            ),
            module(
                "rabbit_misc",
                "rabbit_common",
                "deps/rabbit_common/src/rabbit_misc.erl",
            ),
        ],
        vec![rabbit_header_file()],
    )
    .with_apps(vec![
        manifest("rabbit", &["rabbit_common", "ra", "ssl"]),
        AppManifest {
            name: app("rabbitmq_amqp_client"),
            source: ManifestSource::ErlangMk,
            requires: ManifestDeps::Unreadable(ManifestUnreadable::UnexpandedVariable(
                MakeVariableName::new("PLUGINS").unwrap(),
            )),
        },
        AppManifest {
            name: app("rabbit_common"),
            source: ManifestSource::AppSrc,
            requires: ManifestDeps::Unreadable(ManifestUnreadable::Missing),
        },
        AppManifest {
            name: app("rabbitmq_stomp"),
            source: ManifestSource::AppSrc,
            requires: ManifestDeps::Unreadable(ManifestUnreadable::ParseFailed),
        },
    ])
    .with_cuttlefish(
        vec![SchemaFile {
            path: path("deps/rabbit/priv/schema/rabbit.schema"),
            app: app("rabbit"),
            mappings: conf_keys(&["ssl_options.cacertfile", "tcp_listen_options.backlog"]),
            validators: validators(&["file_accessible", "port"]),
        }],
        vec![PartialFile {
            path: path("deps/rabbit/priv/schema/ssl_options.partial"),
            app: app("rabbit"),
            name: PartialName::new("ssl_options").unwrap(),
            mappings: conf_keys(&["cacertfile", "verify"]),
            validators: validators(&["ssl_versions"]),
        }],
    )
    .into_canonical()
}

fn v5_text(body: &str) -> String {
    format!(
        "# backhopper snapshot\n# format-version: 5\n# project: rabbitmq-server\n# tag: v4.2.8\n# commit: {}\n# scanned-paths: deps/*\n# generated-by: test\n# generated-at: 1970-01-01T00:00:00Z\n\n{body}",
        "0".repeat(40)
    )
}

fn v4_text(body: &str) -> String {
    v5_text(body).replace("format-version: 5", "format-version: 4")
}

#[test]
fn a_snapshot_with_every_new_section_round_trips() {
    let snap = full_snapshot();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert_eq!(format::to_string(&back).unwrap(), text);
}

#[test]
fn sections_are_written_in_canonical_order() {
    let text = format::to_string(&full_snapshot()).unwrap();
    let positions: Vec<usize> = [
        "\napp rabbit\n",
        "\nmodule rabbit_amqqueue\n",
        "\nheader deps/rabbit_common/include/rabbit.hrl\n",
        "\nschema deps/rabbit/priv/schema/rabbit.schema\n",
        "\npartial deps/rabbit/priv/schema/ssl_options.partial\n",
    ]
    .iter()
    .map(|marker| {
        text.find(marker)
            .unwrap_or_else(|| panic!("{marker:?} in:\n{text}"))
    })
    .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{text}");
}

#[test]
fn the_documented_grammar_is_what_is_written() {
    let text = format::to_string(&full_snapshot()).unwrap();
    for line in [
        "app rabbit\n  manifest erlang_mk\n  requires ra\n  requires rabbit_common\n  requires ssl\n",
        "app rabbitmq_amqp_client\n  manifest erlang_mk\n  requires_unreadable unexpanded_variable PLUGINS\n",
        "app rabbit_common\n  manifest app_src\n  requires_unreadable missing\n",
        "app rabbitmq_stomp\n  manifest app_src\n  requires_unreadable parse_failed\n",
        "header deps/rabbit_common/include/rabbit.hrl\n  app rabbit_common\n  include rabbit_common include/resource.hrl\n",
        "    field durable :: boolean() = false\n",
        "    field arguments = []\n",
        "  define DEFAULT_CONSUMER_TIMEOUT 1800000\n  define IS_QUEUE/1 is_record(Q, amqqueue)\n  define ROUTING_KEY_LIMIT ?\n",
        "schema deps/rabbit/priv/schema/rabbit.schema\n  app rabbit\n  mapping ssl_options.cacertfile\n  mapping tcp_listen_options.backlog\n  validator file_accessible\n  validator port\n",
        "partial deps/rabbit/priv/schema/ssl_options.partial\n  app rabbit\n  name ssl_options\n  mapping cacertfile\n  mapping verify\n  validator ssl_versions\n",
        "# release-root: rabbit\n# release-root: rabbitmq_management\n",
        "# dep-pin: ra = hex 2.17.1\n",
    ] {
        assert!(text.contains(line), "missing {line:?} in:\n{text}");
    }
}

#[test]
fn release_roots_and_dep_pins_round_trip_in_the_header() {
    let back = parser::parse(&format::to_string(&full_snapshot()).unwrap()).unwrap();
    let host = back.header().host_facts.as_ref().unwrap();
    assert_eq!(
        host.release_roots.as_slice(),
        [app("rabbit"), app("rabbitmq_management")]
    );
    assert_eq!(host.digest, digest());
    assert_eq!(back.header().dep_pins.len(), 1);
}

#[test]
fn release_roots_are_sorted_when_canonicalized() {
    let mut header = v5_header();
    header.host_facts = Some(HostReleaseFacts {
        release_roots: NonEmpty::new(app("rabbitmq_mqtt"), [app("rabbit"), app("rabbit")]),
        digest: digest(),
    });
    let snap = Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical();
    let roots = &snap.header().host_facts.as_ref().unwrap().release_roots;
    assert_eq!(roots.as_slice(), [app("rabbit"), app("rabbitmq_mqtt")]);
}

#[test]
fn release_roots_without_a_digest_are_refused() {
    let text = v5_text("").replacen("\n\n", "\n# release-root: rabbit\n\n", 1);
    assert!(parser::parse(&text).is_err());
}

#[test]
fn a_digest_without_release_roots_is_refused() {
    let text = v5_text("").replacen(
        "\n\n",
        &format!("\n# host-facts: {}\n\n", "b".repeat(64)),
        1,
    );
    assert!(parser::parse(&text).is_err());
}

#[test]
fn release_roots_out_of_order_are_refused() {
    let text = v5_text("").replacen(
        "\n\n",
        &format!(
            "\n# release-root: rabbitmq_mqtt\n# release-root: rabbit\n# host-facts: {}\n\n",
            "b".repeat(64)
        ),
        1,
    );
    assert!(matches!(
        parser::parse(&text),
        Err(SnapshotError::NotCanonical { .. })
    ));
}

#[test]
fn a_format_5_module_without_an_app_is_refused() {
    let text = v5_text("module rabbit_misc\n  path deps/rabbit_common/src/rabbit_misc.erl\n");
    let err = parser::parse(&text).unwrap_err();
    assert!(err.to_string().contains("no app line"), "{err}");
}

#[test]
fn a_format_5_header_without_an_app_is_refused() {
    let text = v5_text("header deps/rabbit_common/include/rabbit.hrl\n");
    let err = parser::parse(&text).unwrap_err();
    assert!(err.to_string().contains("no app line"), "{err}");
}

#[test]
fn a_format_5_schema_without_an_app_is_refused() {
    let text = v5_text("schema deps/rabbit/priv/schema/rabbit.schema\n  mapping listeners.tcp\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn a_partial_without_a_name_is_refused() {
    let text = v5_text("partial deps/rabbit/priv/schema/ssl_options.partial\n  app rabbit\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn an_app_section_without_a_manifest_line_is_refused() {
    assert!(parser::parse(&v5_text("app rabbit\n  requires ra\n")).is_err());
}

#[test]
fn an_app_marked_unreadable_cannot_also_list_requirements() {
    let text =
        v5_text("app rabbit\n  manifest erlang_mk\n  requires ra\n  requires_unreadable missing\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn the_writer_refuses_a_format_5_module_without_an_app() {
    let snap = Snapshot::from_extracted(
        v5_header(),
        vec![Module::new(ModuleName::new("rabbit_misc").unwrap())],
        Vec::new(),
    )
    .into_canonical();
    assert!(matches!(
        format::to_string(&snap),
        Err(SnapshotError::Unwritable { .. })
    ));
}

#[test]
fn a_define_line_in_a_format_4_file_is_refused() {
    let text = v4_text("header include/rabbit.hrl\n  define DEFAULT_CONSUMER_TIMEOUT 1800000\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn an_include_line_in_a_format_4_file_is_refused() {
    let text = v4_text("header include/rabbit.hrl\n  include rabbit_common include/resource.hrl\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn an_app_line_on_a_format_4_header_is_refused() {
    let text = v4_text("header include/rabbit.hrl\n  app rabbit\n");
    assert!(parser::parse(&text).is_err());
}

#[test]
fn the_new_sections_in_a_format_4_file_are_refused() {
    for body in [
        "app rabbit\n  manifest erlang_mk\n",
        "schema priv/schema/rabbit.schema\n  app rabbit\n",
        "partial priv/schema/ssl_options.partial\n  app rabbit\n  name ssl_options\n",
    ] {
        assert!(parser::parse(&v4_text(body)).is_err(), "{body}");
    }
}

#[test]
fn the_new_header_lines_in_a_format_4_file_are_refused() {
    let with_root = v4_text("").replacen("\n\n", "\n# release-root: rabbit\n\n", 1);
    assert!(parser::parse(&with_root).is_err());
    let with_facts = v4_text("").replacen(
        "\n\n",
        &format!("\n# host-facts: {}\n\n", "b".repeat(64)),
        1,
    );
    assert!(parser::parse(&with_facts).is_err());
}

#[test]
fn a_record_default_in_a_format_4_file_is_refused() {
    let text = v4_text(
        "header include/rabbit.hrl\n  record amqqueue\n    field durable :: boolean() = false\n",
    );
    assert!(parser::parse(&text).is_err());
}

#[test]
fn a_format_4_file_still_parses_and_is_written_back_as_format_4() {
    let text = v4_text("module rabbit_misc\n  path deps/rabbit_common/src/rabbit_misc.erl\n");
    let snap = parser::parse(&text).unwrap();
    assert_eq!(snap.header().format_version, 4);
    assert_eq!(format::to_string(&snap).unwrap(), text);
}

#[test]
fn a_field_default_of_false_round_trips() {
    let text = v5_text(
        "header include/rabbit.hrl\n  app rabbit\n  record amqqueue\n    field durable :: boolean() = false\n",
    );
    let snap = parser::parse(&text).unwrap();
    let durable = &snap.headers()[0].records[0].fields[0];
    assert_eq!(durable.type_repr.as_deref(), Some("boolean()"));
    assert_eq!(durable.default.as_ref().unwrap().as_str(), "false");
    assert_eq!(format::to_string(&snap).unwrap(), text);
}

#[test]
fn an_empty_binary_default_round_trips() {
    let text = v5_text(
        "header include/rabbit.hrl\n  app rabbit\n  record amqqueue\n    field vhost :: binary() = <<\"\">>\n",
    );
    let snap = parser::parse(&text).unwrap();
    let vhost = &snap.headers()[0].records[0].fields[0];
    assert_eq!(vhost.default.as_ref().unwrap().as_str(), "<<\"\">>");
    assert_eq!(format::to_string(&snap).unwrap(), text);
}

#[test]
fn a_multi_line_default_normalises_to_one_line() {
    let default = NormalisedDefault::from_source(
        "#{max_length => 100,\n        %% bytes\n        overflow  =>  drop_head}",
    )
    .unwrap();
    assert_eq!(
        default.as_str(),
        "#{max_length => 100, overflow => drop_head}"
    );
}

#[test]
fn a_default_with_a_line_break_inside_a_string_stays_on_one_line() {
    let default = NormalisedDefault::from_source("\"first\nsecond\"").unwrap();
    assert_eq!(default.as_str(), "\"first\\nsecond\"");
}

#[test]
fn a_blank_default_is_no_default() {
    assert!(NormalisedDefault::from_source(" \n % nothing\n").is_none());
}

#[test]
fn a_default_that_is_not_normalised_is_refused_when_read() {
    assert!(NormalisedDefault::from_stored("#{a  =>  1}").is_err());
    assert!(NormalisedDefault::from_stored("#{a => 1}").is_ok());
}

#[test]
fn a_typed_field_with_a_multi_line_type_keeps_its_default() {
    let text = v5_text(concat!(
        "header include/rabbit.hrl\n  app rabbit\n  record amqqueue\n",
        "    field state :: running |\n",
        "     stopped = running\n",
    ));
    let snap = parser::parse(&text).unwrap();
    let state = &snap.headers()[0].records[0].fields[0];
    assert_eq!(state.default.as_ref().unwrap().as_str(), "running");
    assert_eq!(state.type_repr.as_deref(), Some("running |\n     stopped"));
}

#[test]
fn an_empty_macro_body_is_written_as_a_token_no_erlang_source_holds() {
    let mut hrl = HrlFile::new(path("include/rabbit.hrl"));
    hrl.app = Some(app("rabbit"));
    hrl.defines = vec![text_define("EMPTY", None, "")];
    let snap = Snapshot::from_extracted(v5_header(), Vec::new(), vec![hrl]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(text.contains("  define EMPTY %empty%\n"), "{text}");
    assert_eq!(parser::parse(&text).unwrap(), snap);
}

#[test]
fn a_bare_question_mark_body_is_never_text() {
    let body = NormalisedMacroBody::from_source("?");
    assert_ne!(body.as_str(), "?");
    assert!(NormalisedMacroBody::from_stored("?").is_err());
}

#[test]
fn a_nullary_define_after_a_function_like_one_is_out_of_order() {
    let text = v5_text(
        "header include/rabbit.hrl\n  app rabbit\n  define IS_QUEUE/1 is_record(Q, amqqueue)\n  define IS_QUEUE 10\n",
    );
    let err = parser::parse(&text).unwrap_err();
    assert!(matches!(err, SnapshotError::NotCanonical { .. }), "{err}");
}

#[test]
fn defines_sort_by_name_then_arity_with_none_first() {
    let mut hrl = HrlFile::new(path("include/rabbit.hrl"));
    hrl.app = Some(app("rabbit"));
    hrl.defines = vec![
        text_define("QUEUE", Some(2), "b"),
        text_define("QUEUE", None, "a"),
        text_define("QUEUE", Some(1), "c"),
        text_define("AMQP", None, "d"),
    ];
    let snap = Snapshot::from_extracted(v5_header(), Vec::new(), vec![hrl]).into_canonical();
    let keys: Vec<String> = snap.headers()[0]
        .defines
        .iter()
        .map(|d| d.key.to_string())
        .collect();
    assert_eq!(keys, ["AMQP", "QUEUE", "QUEUE/1", "QUEUE/2"]);
}

#[test]
fn includes_sort_by_app_then_path() {
    let mut hrl = HrlFile::new(path("include/rabbit.hrl"));
    hrl.app = Some(app("rabbit"));
    let include = |a: &str, p: &str| HeaderRef {
        app: app(a),
        app_path: path(p),
    };
    hrl.includes = vec![
        include("rabbit_common", "include/rabbit.hrl"),
        include("kernel", "include/file.hrl"),
        include("rabbit_common", "include/resource.hrl"),
        include("kernel", "include/file.hrl"),
    ];
    let snap = Snapshot::from_extracted(v5_header(), Vec::new(), vec![hrl]).into_canonical();
    let rendered: Vec<String> = snap.headers()[0]
        .includes
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        rendered,
        [
            "kernel/include/file.hrl",
            "rabbit_common/include/rabbit.hrl",
            "rabbit_common/include/resource.hrl"
        ]
    );
}

#[test]
fn apps_out_of_order_are_refused() {
    let text =
        v5_text("app rabbit\n  manifest erlang_mk\n\napp amqp10_client\n  manifest erlang_mk\n");
    assert!(matches!(
        parser::parse(&text),
        Err(SnapshotError::NotCanonical { .. })
    ));
}

#[test]
fn requirements_out_of_order_are_refused() {
    let text = v5_text("app rabbit\n  manifest erlang_mk\n  requires ssl\n  requires ra\n");
    assert!(matches!(
        parser::parse(&text),
        Err(SnapshotError::NotCanonical { .. })
    ));
}

#[test]
fn a_schema_section_before_a_module_section_is_refused() {
    let text = v5_text(
        "schema priv/schema/rabbit.schema\n  app rabbit\n\nmodule rabbit_misc\n  app rabbit\n",
    );
    assert!(matches!(
        parser::parse(&text),
        Err(SnapshotError::NotCanonical { .. })
    ));
}

#[test]
fn mappings_and_validators_out_of_order_are_refused() {
    for body in [
        "schema priv/schema/rabbit.schema\n  app rabbit\n  mapping b\n  mapping a\n",
        "schema priv/schema/rabbit.schema\n  app rabbit\n  validator b\n  validator a\n",
        "schema priv/schema/rabbit.schema\n  app rabbit\n  validator a\n  mapping a\n",
    ] {
        assert!(parser::parse(&v5_text(body)).is_err(), "{body}");
    }
}

#[test]
fn schemas_and_partials_out_of_order_are_refused() {
    let schemas = v5_text("schema b.schema\n  app rabbit\n\nschema a.schema\n  app rabbit\n");
    assert!(parser::parse(&schemas).is_err());
    let partials = v5_text(
        "partial b.partial\n  app rabbit\n  name b\n\npartial a.partial\n  app rabbit\n  name a\n",
    );
    assert!(parser::parse(&partials).is_err());
}

#[test]
fn app_path_is_the_part_after_the_app_root() {
    let mut hrl = HrlFile::new(path("deps/rabbit_common/include/rabbit.hrl"));
    assert_eq!(hrl.app_path(), None);
    hrl.app = Some(app("rabbit_common"));
    assert_eq!(hrl.app_path(), Some(path("include/rabbit.hrl")));
    assert_eq!(
        hrl.header_ref(),
        Some(HeaderRef {
            app: app("rabbit_common"),
            app_path: path("include/rabbit.hrl"),
        })
    );
}

#[test]
fn app_path_of_a_path_already_relative_to_the_app_is_the_path() {
    let mut hrl = HrlFile::new(path("include/ra.hrl"));
    hrl.app = Some(app("ra"));
    assert_eq!(hrl.app_path(), Some(path("include/ra.hrl")));
}

#[test]
fn app_path_ignores_a_file_named_like_the_app() {
    let mut hrl = HrlFile::new(path("lib/kernel/include/kernel"));
    hrl.app = Some(app("kernel"));
    assert_eq!(hrl.app_path(), Some(path("include/kernel")));
}

#[test]
fn the_json_form_of_a_snapshot_without_new_sections_has_no_new_keys() {
    let snap = Snapshot::from_extracted(
        snapshot_header("ra", "v2.17.1"),
        vec![Module::new(ModuleName::new("ra").unwrap())],
        Vec::new(),
    )
    .into_canonical();
    let json = serde_json::to_value(&snap).unwrap();
    let keys: BTreeSet<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, BTreeSet::from(["header", "headers", "modules"]));
    assert!(json["header"].get("host_facts").is_none());
}

#[test]
fn a_snapshot_with_every_new_section_round_trips_through_json() {
    let snap = full_snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: Snapshot<state::Canonical> = serde_json::from_str(&json).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn json_apps_out_of_order_are_not_canonical() {
    let snap = full_snapshot();
    let mut json = serde_json::to_value(&snap).unwrap();
    json["apps"].as_array_mut().unwrap().reverse();
    let result: Result<Snapshot<state::Canonical>, _> = serde_json::from_value(json);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("apps out of order")
    );
}

#[test]
fn json_format_5_module_without_an_app_is_not_canonical() {
    let mut header = v5_header();
    header.format_version = 5;
    let unsorted = Snapshot::from_extracted(
        header,
        vec![Module::new(ModuleName::new("rabbit_misc").unwrap())],
        Vec::new(),
    );
    let json = serde_json::to_value(&unsorted).unwrap();
    let result: Result<Snapshot<state::Canonical>, _> = serde_json::from_value(json);
    assert!(result.unwrap_err().to_string().contains("no app"));
}

#[test]
fn app_entries_out_of_class_order_are_refused() {
    for body in [
        "app rabbit\n  requires ra\n  manifest erlang_mk\n",
        "app rabbit\n  requires_unreadable missing\n  manifest erlang_mk\n",
        "app rabbit\n  manifest erlang_mk\n  requires_unreadable missing\n  requires ra\n",
    ] {
        assert!(
            matches!(
                parser::parse(&v5_text(body)),
                Err(SnapshotError::NotCanonical { .. })
            ),
            "{body}"
        );
    }
}

#[test]
fn the_extraction_settings_digest_round_trips_in_the_header() {
    let mut header = v5_header();
    header.extraction_settings = Some(ExtractionSettingsDigest::new("b".repeat(64)).unwrap());
    let snap = Snapshot::<state::Unsorted>::from_extracted(header, Vec::new(), Vec::new())
        .into_canonical();
    let text = format::to_string(&snap).unwrap();
    assert!(text.contains(&format!("# extraction-settings: {}\n", "b".repeat(64))));
    assert_eq!(parser::parse(&text).unwrap(), snap);
}

#[test]
fn a_format_5_header_without_the_digest_reads_as_none() {
    let back = parser::parse(&v5_text("")).unwrap();
    assert_eq!(back.header().extraction_settings, None);
}

#[test]
fn an_extraction_settings_line_in_a_format_4_file_is_refused() {
    let text = v4_text("").replace(
        "# generated-by: test\n",
        &format!(
            "# generated-by: test\n# extraction-settings: {}\n",
            "b".repeat(64)
        ),
    );
    assert!(parser::parse(&text).is_err());
}

#[test]
fn a_malformed_extraction_settings_digest_is_refused() {
    let text = v5_text("").replace(
        "# generated-by: test\n",
        "# generated-by: test\n# extraction-settings: not-hex\n",
    );
    assert!(parser::parse(&text).is_err());
}
