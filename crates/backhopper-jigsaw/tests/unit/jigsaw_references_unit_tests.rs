// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::jigsaw::plugin_set::PluginFile;
use backhopper_core::jigsaw::references::{
    CallForm, NotChecked, Reference, ReferenceSet, SiteDetail,
};
use backhopper_core::model::names::{Arity, ConfKey, PartialName, ValidatorName};
use backhopper_core::model::schema_files::PartialRef;
use backhopper_core::model::snapshot::HeaderRef;
use backhopper_core::model::verdict::IndirectCallForm;
use backhopper_jigsaw::collect;
use backhopper_test_support::{SnapshotBuilder, defining, hrl};

use crate::references_fixtures::{WORKER, lines, rabbit_host};
use crate::support::{
    AppSpec, Build, app, file, lines as site_lines, macro_key, mfa, plugin_set, sites,
};

const WORKER_TEXT: &[&str] = &[
    "-module(rabbit_shovel_extra_worker).",
    "-behaviour(rabbit_backing_queue).",
    "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
    "-export([run/1, limit/0, fetch/1]).",
    "-spec fetch(node()) -> rabbit_backing_queue:state().",
    "",
    "run(Name) ->",
    "    case rabbit_amqqueue:lookup(Name) of",
    "        {ok, Q} -> Q#amqqueue.durable;",
    "        {error, not_found} -> false",
    "    end.",
    "",
    "limit() -> ?ROUTING_KEY_LIMIT.",
    "",
    "fetch(Node) ->",
    "    erpc:call(Node, rabbit_misc, pget, [foo, []]),",
    "    F = fun rabbit_misc:pget/2,",
    "    F.",
];

fn collect_from(apps: &[AppSpec<'_>]) -> ReferenceSet {
    let build = rabbit_host();
    collect(&plugin_set(apps), &build.surface()).unwrap()
}

fn extra(files: &[(&str, &str)]) -> ReferenceSet {
    collect_from(&[AppSpec {
        name: "rabbitmq_shovel_extra",
        requires: &["rabbit_common"],
        files,
    }])
}

fn worker_only(text: &str) -> ReferenceSet {
    extra(&[(WORKER, text)])
}

fn unchecked_lines(references: &ReferenceSet) -> Vec<(NotChecked, u32, String)> {
    references
        .unchecked()
        .iter()
        .map(|site| (site.reason(), site.line, site.symbol.to_string()))
        .collect()
}

fn plugin_file(path: &str) -> PluginFile {
    file("rabbitmq_shovel_extra", path)
}

#[test]
fn every_kind_of_reference_is_found_with_its_line() {
    let text = lines(WORKER_TEXT);
    let schema = lines(&[
        "{mapping, \"shovel.extra.port\", \"rabbitmq_shovel_extra.port\", [{datatype, integer}, {validators, [\"port\"]}]}.",
        "{include_partial, {rabbit, \"ssl_options\"}, [{exclude, [\"cacertfile\"]}]}.",
    ]);
    let references = extra(&[
        (WORKER, &text),
        ("priv/schema/rabbitmq_shovel_extra.schema", &schema),
    ]);

    assert_eq!(
        lines_of(
            &references,
            &Reference::Call(mfa("rabbit_amqqueue:lookup/1"))
        ),
        [8]
    );
    let erpc = sites(&references, &Reference::Call(mfa("rabbit_misc:pget/2")));
    assert_eq!(erpc.len(), 2);
    assert!(erpc.iter().any(|s| s.line == 16
        && s.detail == SiteDetail::Call(CallForm::Indirect(IndirectCallForm::ErpcCall))));
    assert!(
        erpc.iter()
            .any(|s| s.line == 17 && s.detail == SiteDetail::Call(CallForm::FunRef))
    );
    let header = HeaderRef {
        app: app("rabbit_common"),
        app_path: "include/rabbit.hrl".parse().unwrap(),
    };
    assert_eq!(lines_of(&references, &Reference::Header(header)), [3]);
    assert_eq!(
        lines_of(
            &references,
            &Reference::Macro {
                includer: plugin_file(WORKER),
                key: macro_key("ROUTING_KEY_LIMIT", None),
            }
        ),
        [13]
    );
    assert_eq!(
        lines_of(
            &references,
            &Reference::Record {
                includer: plugin_file(WORKER),
                name: "amqqueue".parse().unwrap(),
            }
        ),
        [9]
    );
    assert_eq!(
        lines_of(
            &references,
            &Reference::Behaviour {
                implementer: "rabbit_shovel_extra_worker".parse().unwrap(),
                behaviour: "rabbit_backing_queue".parse().unwrap(),
            }
        ),
        [2]
    );
    assert_eq!(
        lines_of(
            &references,
            &Reference::Type {
                module: "rabbit_backing_queue".parse().unwrap(),
                name: "state".parse().unwrap(),
                arity: Arity::new(0),
            }
        ),
        [5]
    );
    let partial = PartialRef {
        app: app("rabbit"),
        name: PartialName::new("ssl_options").unwrap(),
    };
    assert_eq!(
        lines_of(
            &references,
            &Reference::Partial {
                partial,
                keys: [ConfKey::new("cacertfile").unwrap()].into(),
            }
        ),
        [2]
    );
    assert_eq!(
        lines_of(
            &references,
            &Reference::Validator(ValidatorName::new("port").unwrap())
        ),
        [1]
    );
    assert_eq!(
        lines_of(&references, &Reference::App(app("rabbit_common"))),
        [1]
    );
}

fn code_references(references: &ReferenceSet) -> Vec<&Reference> {
    references
        .external()
        .keys()
        .filter(|reference| !matches!(reference, Reference::App(_)))
        .collect()
}

fn lines_of(references: &ReferenceSet, reference: &Reference) -> Vec<u32> {
    site_lines(references, reference)
}

#[test]
fn a_call_between_two_apps_of_the_plugin_set_is_internal() {
    let references = collect_from(&[
        AppSpec {
            name: "rabbitmq_shovel_extra",
            requires: &["rabbitmq_shovel_core"],
            files: &[(
                WORKER,
                "-module(rabbit_shovel_extra_worker).\nrun() -> rabbit_shovel_core:start(1).\n",
            )],
        },
        AppSpec {
            name: "rabbitmq_shovel_core",
            requires: &[],
            files: &[(
                "src/rabbit_shovel_core.erl",
                "-module(rabbit_shovel_core).\n-export([start/1]).\nstart(N) -> N.\n",
            )],
        },
    ]);
    assert!(
        !references
            .external()
            .contains_key(&Reference::Call(mfa("rabbit_shovel_core:start/1")))
    );
    assert_eq!(references.not_checked()[&NotChecked::Internal], 1);
}

#[test]
fn a_call_with_a_variable_module_is_dynamic() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "run(M, Q) -> M:lookup(Q).",
        "other(M, F) -> apply(M, F, []).",
    ]));
    assert!(code_references(&references).is_empty());
    assert_eq!(
        unchecked_lines(&references),
        vec![
            (NotChecked::Dynamic, 2, "_:lookup/1".to_owned()),
            (NotChecked::Dynamic, 3, "_:_/0".to_owned()),
        ]
    );
}

#[test]
fn a_withheld_indirect_call_is_listed_with_its_line() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "drain(Node, Args) ->",
        "    rpc:call(Node, rabbit_maintenance, drain, Args).",
    ]));
    assert_eq!(
        unchecked_lines(&references),
        vec![(
            NotChecked::Dynamic,
            3,
            "rabbit_maintenance:drain/?".to_owned()
        )]
    );
}

#[test]
fn a_dynamic_call_in_a_test_only_region_is_not_listed() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-ifdef(TEST).",
        "run(M, Q) -> M:lookup(Q).",
        "-endif.",
    ]));
    assert!(references.unchecked().is_empty());
}

#[test]
fn a_macro_the_plugin_set_defines_itself_is_internal() {
    let references = extra(&[
        (
            WORKER,
            &lines(&[
                "-module(rabbit_shovel_extra_worker).",
                "-include(\"shovel_extra.hrl\").",
                "limit() -> ?EXTRA_LIMIT.",
            ]),
        ),
        ("include/shovel_extra.hrl", "-define(EXTRA_LIMIT, 10).\n"),
    ]);
    assert!(
        !references
            .external()
            .keys()
            .any(|r| matches!(r, Reference::Macro { .. }))
    );
}

#[test]
fn a_partial_the_plugin_set_ships_resolves_inside_it() {
    let references = extra(&[
        (
            "priv/schema/rabbitmq_shovel_extra.schema",
            "{include_partial, {rabbitmq_shovel_extra, \"limits\"}, []}.\n",
        ),
        (
            "priv/schema/limits.partial",
            "{mapping, \"max\", \"rabbitmq_shovel_extra.max\", [{datatype, integer}]}.\n",
        ),
    ]);
    assert!(
        !references
            .external()
            .keys()
            .any(|r| matches!(r, Reference::Partial { .. }))
    );
}

#[test]
fn an_imported_call_becomes_an_external_call() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-import(rabbit_misc, [pget/2]).",
        "run(Props) -> pget(durable, Props).",
    ]));
    assert_eq!(
        lines_of(&references, &Reference::Call(mfa("rabbit_misc:pget/2"))),
        [3]
    );
}

#[test]
fn a_call_inside_a_host_logging_macro_is_found_through_the_synthetic_header() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
        "run() -> ?RABBIT_LOG(info, \"started\").",
    ]));
    assert_eq!(
        lines_of(&references, &Reference::Call(mfa("rabbit_log:log/2"))),
        [3]
    );
}

#[test]
fn a_macro_defined_in_a_header_two_includes_away_is_anchored_to_that_header() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
        "limit() -> ?ROUTING_KEY_LIMIT.",
    ]));
    let includes = references
        .host_includes(&plugin_file(WORKER))
        .expect("host includes");
    assert_eq!(includes.len(), 1);
    assert_eq!(
        includes.iter().next().unwrap().to_string(),
        "rabbit_common/include/rabbit.hrl"
    );
}

#[test]
fn a_record_from_an_included_header_that_is_never_used_is_not_referenced() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
        "run() -> ok.",
    ]));
    assert!(
        !references
            .external()
            .keys()
            .any(|r| matches!(r, Reference::Record { .. } | Reference::Macro { .. }))
    );
}

#[test]
fn a_synthetic_header_under_an_app_src_directory_is_found_through_include_lib() {
    let snapshot = SnapshotBuilder::new("rabbitmq-server", "v4.2.8")
        .roots(&["rabbit"])
        .app("rabbit", &[])
        .header(defining(
            hrl("rabbit", "deps/rabbit/src/rabbit_log.hrl"),
            "LOG_NOW",
            None,
            "rabbit_log:now()",
        ))
        .build();
    let build = Build::new(snapshot, &["rabbit"]);
    let references = collect(
        &plugin_set(&[AppSpec {
            name: "rabbitmq_shovel_extra",
            requires: &[],
            files: &[(
                WORKER,
                "-module(rabbit_shovel_extra_worker).\n-include_lib(\"rabbit/src/rabbit_log.hrl\").\nrun() -> ?LOG_NOW.\n",
            )],
        }]),
        &build.surface(),
    )
    .unwrap();
    assert_eq!(
        lines_of(&references, &Reference::Call(mfa("rabbit_log:now/0"))),
        [3]
    );
}

#[test]
fn a_header_of_the_plugin_set_wins_over_a_host_header_of_the_same_name() {
    let references = extra(&[
        (
            WORKER,
            &lines(&[
                "-module(rabbit_shovel_extra_worker).",
                "-include(\"rabbit.hrl\").",
                "limit() -> ?ROUTING_KEY_LIMIT.",
            ]),
        ),
        ("include/rabbit.hrl", "-define(ROUTING_KEY_LIMIT, 1).\n"),
    ]);
    assert!(
        !references
            .external()
            .keys()
            .any(|r| matches!(r, Reference::Macro { .. }))
    );
}

#[test]
fn a_host_macro_used_in_a_plugin_header_is_sited_at_the_header_line() {
    let references = extra(&[
        (
            WORKER,
            &lines(&[
                "-module(rabbit_shovel_extra_worker).",
                "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
                "-include(\"limits.hrl\").",
            ]),
        ),
        (
            "include/limits.hrl",
            "%% limits\n-define(LIMIT, ?ROUTING_KEY_LIMIT).\n",
        ),
    ]);
    let used = sites(
        &references,
        &Reference::Macro {
            includer: plugin_file(WORKER),
            key: macro_key("ROUTING_KEY_LIMIT", None),
        },
    );
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].file, plugin_file("include/limits.hrl"));
    assert_eq!(used[0].line, 2);
}

#[test]
fn an_include_nothing_can_resolve_is_counted_once_per_including_file() {
    let references = extra(&[
        (
            WORKER,
            &lines(&[
                "-module(rabbit_shovel_extra_worker).",
                "-include_lib(\"kernel/include/logger.hrl\").",
                "-include_lib(\"kernel/include/file.hrl\").",
            ]),
        ),
        (
            "src/rabbit_shovel_extra_sup.erl",
            "-module(rabbit_shovel_extra_sup).\n-include_lib(\"kernel/include/logger.hrl\").\n",
        ),
    ]);
    assert_eq!(references.not_checked()[&NotChecked::UnresolvedInclude], 2);
}

#[test]
fn a_call_guarded_by_a_version_conditional_is_not_checked() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-if(?OTP_RELEASE >= 28).",
        "run() -> json:encode(1).",
        "-else.",
        "run() -> ok.",
        "-endif.",
    ]));
    assert!(code_references(&references).is_empty());
    assert_eq!(references.not_checked()[&NotChecked::ConditionalSite], 1);
}

#[test]
fn a_call_with_one_unguarded_site_is_still_checked() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-if(?OTP_RELEASE >= 28).",
        "run() -> json:encode(1).",
        "-endif.",
        "other() -> json:encode(2).",
    ]));
    assert_eq!(
        lines_of(&references, &Reference::Call(mfa("json:encode/1"))),
        [3, 5]
    );
}

#[test]
fn code_under_a_test_conditional_is_not_read() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-ifdef(TEST).",
        "run() -> meck:new(rabbit_misc).",
        "-endif.",
        "ok() -> ok.",
    ]));
    assert!(code_references(&references).is_empty());
    assert!(references.not_checked().is_empty());
}

#[test]
fn a_quoted_elixir_module_call_is_found() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "run() -> 'Elixir.RabbitMQ.CLI':main([]).",
    ]));
    assert_eq!(
        references
            .external()
            .keys()
            .filter(|r| matches!(r, Reference::Call(m) if m.module.as_str().starts_with("Elixir.")))
            .count(),
        1
    );
}

#[test]
fn a_construction_records_the_fields_it_leaves_to_the_default() {
    let references = worker_only(&lines(&[
        "-module(rabbit_shovel_extra_worker).",
        "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
        "make(N) -> #amqqueue{name = N}.",
        "update(Q) -> Q#amqqueue{durable = true}.",
        "match(#amqqueue{name = N}) -> N.",
    ]));
    let used = sites(
        &references,
        &Reference::Record {
            includer: plugin_file(WORKER),
            name: "amqqueue".parse().unwrap(),
        },
    );
    let constructions: Vec<_> = used
        .iter()
        .filter_map(|site| match &site.detail {
            SiteDetail::Construction(c) => Some((site.line, c.unset.len())),
            _ => None,
        })
        .collect();
    assert_eq!(constructions, [(3, 2)]);
    assert_eq!(used.len(), 3);
}
