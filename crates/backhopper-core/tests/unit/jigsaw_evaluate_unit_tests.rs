// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::path::PathBuf;

use backhopper_core::jigsaw::evaluate;
use backhopper_core::jigsaw::findings::{Finding, Warning, WithheldReason, WithheldSymbol};
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginFile, PluginSet, PluginSources};
use backhopper_core::jigsaw::references::{
    DynamicCall, NotChecked, Reference, ReferenceSet, ReferenceSite, SiteDetail, UncheckedSite,
    UncheckedSymbol,
};
use backhopper_core::jigsaw::report::{FindingSpan, HostIdx, JigsawCell, JigsawReport, OtpIdx};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::macro_decl::MacroKey;
use backhopper_core::model::names::{ApplicationName, Arity, Mfa, RelativePath};
use backhopper_core::model::schema_files::PartialRef;
use backhopper_core::model::snapshot::{FunArity, HeaderRef, HrlFile};
use backhopper_test_support::{
    SnapshotBuilder, defining, defining_conditional, hrl, loaded_plan, module_with, with_callbacks,
    with_exported_types, with_record, with_spec,
};

const RELEASES: [&str; 4] = ["v4.2.0", "v4.2.1", "v4.2.2", "v4.2.3"];

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn base(tag: &str) -> SnapshotBuilder {
    SnapshotBuilder::new("rabbitmq-server", tag)
        .roots(&["rabbit"])
        .app("rabbit", &["rabbit_common"])
        .app("rabbit_common", &[])
}

fn host(
    build: impl Fn(usize, SnapshotBuilder) -> SnapshotBuilder,
) -> Vec<(&'static str, SnapshotBuilder)> {
    RELEASES
        .iter()
        .enumerate()
        .map(|(index, tag)| (*tag, build(index, base(tag))))
        .collect()
}

fn kernel(tag: &str) -> SnapshotBuilder {
    SnapshotBuilder::new("otp", tag)
        .app("kernel", &[])
        .module("kernel", module_with("lists", &[("map", 2)]))
}

fn plugin(name: &str) -> PluginSet {
    PluginSet::new(PluginSources {
        apps: vec![PluginAppSources {
            root: PathBuf::from(format!("/plugins/{name}")),
            manifest: AppManifest {
                name: app(name),
                source: ManifestSource::AppSrc,
                requires: ManifestDeps::Listed(BTreeSet::new()),
            }
            .into_read()
            .unwrap(),
            manifest_path: RelativePath::new(format!("src/{name}.app.src")).unwrap(),
            files: Vec::new(),
        }],
        host_provided: BTreeSet::new(),
    })
    .unwrap()
}

fn site(line: u32) -> ReferenceSite {
    ReferenceSite {
        file: file("src/rabbit_shovel_extra_worker.erl"),
        line,
        detail: SiteDetail::Plain,
    }
}

fn file(path: &str) -> PluginFile {
    PluginFile {
        app: app("rabbitmq_shovel_extra"),
        path: RelativePath::new(path).unwrap(),
    }
}

fn run(
    host_content: &[(&str, SnapshotBuilder)],
    otp: &[(&str, SnapshotBuilder)],
    majors: &[u32],
    references: ReferenceSet,
) -> JigsawReport {
    let loaded = loaded_plan(host_content, otp, majors);
    evaluate(&loaded, &plugin("rabbitmq_shovel_extra"), |_, _| {
        Ok::<_, Infallible>(references)
    })
    .unwrap()
}

fn run_one_line(
    host_content: &[(&str, SnapshotBuilder)],
    references: ReferenceSet,
) -> JigsawReport {
    run(
        host_content,
        &[
            ("OTP-27.3", kernel("OTP-27.3")),
            ("OTP-27.3.1", kernel("OTP-27.3.1")),
        ],
        &[27],
        references,
    )
}

fn reference_to(reference: Reference, line: u32) -> ReferenceSet {
    let mut references = ReferenceSet::new();
    references.add(reference, site(line));
    references
}

fn call(text: &str) -> Reference {
    Reference::Call(text.parse::<Mfa>().unwrap())
}

fn host_span(report: &JigsawReport, positions: &[usize]) -> FindingSpan {
    let indices: Vec<HostIdx> = report.targets().host_indices().collect();
    FindingSpan::Host(positions.iter().map(|&p| indices[p]).collect())
}

fn otp_span(report: &JigsawReport, positions: &[usize]) -> FindingSpan {
    let indices: Vec<OtpIdx> = report.targets().otp_indices().collect();
    FindingSpan::Otp(positions.iter().map(|&p| indices[p]).collect())
}

fn findings(report: &JigsawReport) -> Vec<(&Finding, &FindingSpan)> {
    report
        .findings()
        .iter()
        .map(|ranged| (&ranged.finding, &ranged.span))
        .collect()
}

fn rabbit_hrl() -> HrlFile {
    hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl")
}

fn rabbit_header() -> HeaderRef {
    HeaderRef {
        app: app("rabbit_common"),
        app_path: RelativePath::new("include/rabbit.hrl").unwrap(),
    }
}

fn with_host_include(mut references: ReferenceSet, includer: &str) -> ReferenceSet {
    references.set_host_includes(file(includer), BTreeSet::from([rabbit_header()]));
    references
}

#[test]
fn a_removed_function_names_the_arities_the_target_still_has() {
    let content = host(|index, builder| {
        let exports: &[(&str, u8)] = if index >= 2 {
            &[("lookup", 1), ("lookup_many", 2)]
        } else {
            &[("lookup", 1), ("lookup_many", 1)]
        };
        builder.module("rabbit", module_with("rabbit_amqqueue", exports))
    });
    let report = run_one_line(
        &content,
        reference_to(call("rabbit_amqqueue:lookup_many/2"), 12),
    );
    assert_eq!(findings(&report).len(), 1);
    let (finding, span) = findings(&report)[0];
    assert_eq!(
        finding,
        &Finding::FunctionMissing {
            mfa: "rabbit_amqqueue:lookup_many/2".parse().unwrap(),
            other_arities: BTreeSet::from([Arity::new(1)]),
        }
    );
    assert_eq!(span, &host_span(&report, &[0, 1]));
    assert!(!report.is_clean());
}

#[test]
fn a_module_missing_at_a_release_is_reported_once_for_all_its_functions() {
    let content = host(|index, builder| {
        if index == 0 {
            builder
        } else {
            builder.module(
                "rabbit",
                module_with("rabbit_amqqueue", &[("lookup", 1), ("declare", 4)]),
            )
        }
    });
    let mut references = reference_to(call("rabbit_amqqueue:lookup/1"), 8);
    references.add(call("rabbit_amqqueue:declare/4"), site(9));
    let report = run_one_line(&content, references);
    assert_eq!(report.findings().len(), 1);
    assert!(matches!(
        report.findings()[0].finding,
        Finding::ModuleMissing { .. }
    ));
    assert_eq!(report.findings()[0].sites.as_slice().len(), 2);
}

#[test]
fn a_record_with_a_field_inserted_is_reported_where_it_differs() {
    let content = host(|index, builder| {
        let fields: &[(&str, Option<&str>)] = if index == 2 {
            &[
                ("name", None),
                ("durable", Some("false")),
                ("vhost", None),
                ("pid", None),
            ]
        } else {
            &[("name", None), ("durable", Some("false")), ("pid", None)]
        };
        builder.header(with_record(rabbit_hrl(), "amqqueue", fields))
    });
    let references = with_host_include(
        reference_to(
            Reference::Record {
                includer: file("src/rabbit_shovel_extra_worker.erl"),
                name: "amqqueue".parse().unwrap(),
            },
            9,
        ),
        "src/rabbit_shovel_extra_worker.erl",
    );
    let report = run_one_line(&content, references);
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(
        found[0].0,
        Finding::RecordFieldsChanged { build, target, .. } if build.len() == 3 && target.len() == 4
    ));
    assert_eq!(found[0].1, &host_span(&report, &[2]));
}

#[test]
fn a_record_field_whose_type_changed_gives_nothing() {
    let content = host(|index, builder| {
        let mut file = with_record(
            rabbit_hrl(),
            "amqqueue",
            &[("name", None), ("durable", Some("false"))],
        );
        if index == 1 {
            file.records[0].fields[1].type_repr = Some("boolean()".to_owned());
        }
        builder.header(file)
    });
    let references = with_host_include(
        reference_to(
            Reference::Record {
                includer: file("src/rabbit_shovel_extra_worker.erl"),
                name: "amqqueue".parse().unwrap(),
            },
            9,
        ),
        "src/rabbit_shovel_extra_worker.erl",
    );
    let report = run_one_line(&content, references);
    assert!(report.is_clean());
}

#[test]
fn a_macro_with_a_conditional_body_is_withheld_not_reported() {
    let content = host(|index, builder| {
        let file = match index {
            1 => defining_conditional(rabbit_hrl(), "ROUTING_KEY_LIMIT", None),
            0 => defining(rabbit_hrl(), "ROUTING_KEY_LIMIT", None, "256"),
            _ => defining(rabbit_hrl(), "ROUTING_KEY_LIMIT", None, "255"),
        };
        builder.header(file)
    });
    let references = with_host_include(
        reference_to(
            Reference::Macro {
                includer: file("src/rabbit_shovel_extra_worker.erl"),
                key: MacroKey::object_like("ROUTING_KEY_LIMIT".parse().unwrap()),
            },
            13,
        ),
        "src/rabbit_shovel_extra_worker.erl",
    );
    let report = run_one_line(&content, references);
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(found[0].0, Finding::MacroChanged { .. }));
    assert_eq!(found[0].1, &host_span(&report, &[0]));
    assert_eq!(
        report.tallies().withheld()[&WithheldReason::ConditionalMacro],
        1
    );
    let [withheld] = report.withheld() else {
        panic!("one withheld symbol expected");
    };
    assert!(matches!(
        &withheld.symbol,
        WithheldSymbol::ConditionalMacro { key, .. } if key.to_string() == "ROUTING_KEY_LIMIT"
    ));
    assert_eq!(withheld.span, host_span(&report, &[1]));
    assert_eq!(withheld.sites.as_slice().len(), 1);
}

#[test]
fn a_macro_a_used_macro_names_is_compared_too() {
    let content = host(|index, builder| {
        let outer = defining(rabbit_hrl(), "OUTER", None, "?INNER + 1");
        let inner_body = if index == 1 { "2" } else { "1" };
        builder.header(defining(outer, "INNER", None, inner_body))
    });
    let references = with_host_include(
        reference_to(
            Reference::Macro {
                includer: file("src/rabbit_shovel_extra_worker.erl"),
                key: MacroKey::object_like("OUTER".parse().unwrap()),
            },
            3,
        ),
        "src/rabbit_shovel_extra_worker.erl",
    );
    let report = run_one_line(&content, references);
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(
        found[0].0,
        Finding::MacroChanged { key, .. } if key.name.as_str() == "INNER"
    ));
}

fn behaviour_reference(exports: &[(&str, u8)]) -> ReferenceSet {
    let mut references = reference_to(
        Reference::Behaviour {
            implementer: "rabbit_shovel_extra_queue".parse().unwrap(),
            behaviour: "rabbit_backing_queue".parse().unwrap(),
        },
        2,
    );
    references.set_exports(
        "rabbit_shovel_extra_queue".parse().unwrap(),
        exports
            .iter()
            .map(|(name, arity)| FunArity {
                name: name.parse().unwrap(),
                arity: Arity::new(*arity),
            })
            .collect(),
    );
    references
}

fn behaviour_host() -> Vec<(&'static str, SnapshotBuilder)> {
    host(|index, builder| {
        let callbacks: &[(&str, u8)] = match index {
            0 => &[("init", 3)],
            1 => &[("init", 3), ("terminate", 2), ("drop_pending", 1)],
            _ => &[("init", 3), ("terminate", 2)],
        };
        builder.module(
            "rabbit",
            with_callbacks(module_with("rabbit_backing_queue", &[]), callbacks, &[]),
        )
    })
}

#[test]
fn a_new_required_callback_is_reported_when_the_implementer_does_not_export_it() {
    let report = run_one_line(
        &behaviour_host(),
        behaviour_reference(&[("init", 3), ("terminate", 2)]),
    );
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(
        found[0].0,
        Finding::CallbackRequired { callback, .. } if callback.name.as_str() == "drop_pending"
    ));
    assert_eq!(found[0].1, &host_span(&report, &[1]));
}

#[test]
fn a_new_required_callback_the_implementer_exports_and_a_removed_one_give_nothing() {
    let report = run_one_line(
        &behaviour_host(),
        behaviour_reference(&[("init", 3), ("terminate", 2), ("drop_pending", 1)]),
    );
    assert!(report.is_clean());
}

#[test]
fn a_reference_the_build_patch_does_not_satisfy_is_counted_and_never_a_finding() {
    let content = host(|_, builder| {
        builder.module("rabbit", module_with("rabbit_amqqueue", &[("lookup", 1)]))
    });
    let mut references = reference_to(call("rabbit_amqqueue:lookup_removed/9"), 4);
    references.add(call("lists:nosuch/1"), site(5));
    let report = run_one_line(&content, references);
    assert!(report.is_clean());
    assert_eq!(
        report.tallies().not_checked()[&NotChecked::UnresolvedAtBuild],
        2
    );
    assert_eq!(report.tallies().checked(), 0);
}

fn listed(report: &JigsawReport, reason: NotChecked) -> Vec<(u32, String)> {
    report
        .unchecked()
        .iter()
        .filter(|site| site.reason() == reason)
        .map(|site| (site.line, site.symbol.to_string()))
        .collect()
}

fn dynamic_site(line: u32, module: Option<&str>, function: &str) -> UncheckedSite {
    UncheckedSite {
        symbol: UncheckedSymbol::Dynamic(DynamicCall {
            module: module.map(|m| m.parse().unwrap()),
            function: Some(function.parse().unwrap()),
            arity: Some(Arity::new(0)),
        }),
        file: file("src/rabbit_shovel_extra_worker.erl"),
        line,
    }
}

#[test]
fn an_unresolved_call_is_listed_at_every_site_in_order() {
    let content =
        host(|_, builder| builder.module("rabbit", module_with("rabbit_misc", &[("pget", 2)])));
    let mut references = reference_to(call("rabbit_misc:no_such_helper/1"), 12);
    references.add(call("rabbit_misc:no_such_helper/1"), site(4));
    references.add(call("rabbit_misc:no_such_helper/1"), site(4));
    let report = run_one_line(&content, references);
    assert_eq!(
        listed(&report, NotChecked::UnresolvedAtBuild),
        vec![
            (4, "rabbit_misc:no_such_helper/1".to_owned()),
            (12, "rabbit_misc:no_such_helper/1".to_owned()),
        ]
    );
    assert_eq!(
        report.tallies().not_checked()[&NotChecked::UnresolvedAtBuild],
        2
    );
}

#[test]
fn an_unresolved_record_and_macro_are_listed_where_they_are_used() {
    let content = host(|_, builder| builder.header(rabbit_hrl()));
    let includer = file("src/rabbit_shovel_extra_worker.erl");
    let mut references = with_host_include(
        reference_to(
            Reference::Record {
                includer: includer.clone(),
                name: "no_such_record".parse().unwrap(),
            },
            9,
        ),
        "src/rabbit_shovel_extra_worker.erl",
    );
    references.add(
        Reference::Macro {
            includer,
            key: MacroKey::object_like("NO_SUCH_MACRO".parse().unwrap()),
        },
        site(13),
    );
    let report = run_one_line(&content, references);
    assert_eq!(
        listed(&report, NotChecked::UnresolvedAtBuild),
        vec![
            (9, "#no_such_record{}".to_owned()),
            (13, "?NO_SUCH_MACRO".to_owned()),
        ]
    );
}

#[test]
fn a_use_in_a_file_with_an_unresolved_include_is_not_listed() {
    let content = host(|_, builder| builder.header(rabbit_hrl()));
    let includer = file("src/rabbit_shovel_extra_worker.erl");
    let mut references = reference_to(
        Reference::Record {
            includer: includer.clone(),
            name: "no_such_record".parse().unwrap(),
        },
        9,
    );
    references.mark_unresolved_include(includer);
    let report = run_one_line(&content, references);
    assert!(report.unchecked().is_empty());
    assert_eq!(
        report.tallies().not_checked()[&NotChecked::UnresolvedAtBuild],
        0
    );
}

#[test]
fn dynamic_sites_are_listed_by_line_and_counted_as_listed() {
    let content =
        host(|_, builder| builder.module("rabbit", module_with("rabbit_misc", &[("pget", 2)])));
    let mut references = reference_to(call("rabbit_misc:no_such_helper/1"), 7);
    references.add_unchecked(dynamic_site(30, None, "other_missing_helper"));
    references.add_unchecked(dynamic_site(21, Some("rabbit_misc"), "drain"));
    references.add_unchecked(dynamic_site(21, Some("rabbit_misc"), "drain"));
    let report = run_one_line(&content, references);
    assert_eq!(
        listed(&report, NotChecked::Dynamic),
        vec![
            (21, "rabbit_misc:drain/0".to_owned()),
            (30, "_:other_missing_helper/0".to_owned()),
        ]
    );
    for reason in [NotChecked::Dynamic, NotChecked::UnresolvedAtBuild] {
        assert_eq!(
            report.tallies().not_checked()[&reason],
            listed(&report, reason).len()
        );
    }
}

#[test]
fn unchecked_sites_order_by_reason_then_file_then_line() {
    let content =
        host(|_, builder| builder.module("rabbit", module_with("rabbit_misc", &[("pget", 2)])));
    let mut references = reference_to(call("rabbit_misc:no_such_helper/1"), 2);
    references.add_unchecked(dynamic_site(1, None, "late"));
    let report = run_one_line(&content, references);
    let reasons: Vec<NotChecked> = report
        .unchecked()
        .iter()
        .map(UncheckedSite::reason)
        .collect();
    assert_eq!(
        reasons,
        vec![NotChecked::Dynamic, NotChecked::UnresolvedAtBuild]
    );
}

fn spec_host(build: &str, other: &str, odd_index: usize) -> Vec<(&'static str, SnapshotBuilder)> {
    let (build, other) = (build.to_owned(), other.to_owned());
    host(move |index, builder| {
        let spec = if index == odd_index { &other } else { &build };
        builder.module(
            "rabbit_common",
            with_spec(
                module_with("rabbit_misc", &[("start", 1)]),
                "start",
                1,
                spec,
            ),
        )
    })
}

#[test]
fn a_return_shape_that_no_longer_fits_is_reported() {
    let content = spec_host("(term()) -> {ok, pid()}", "(term()) -> pid()", 1);
    let report = run_one_line(&content, reference_to(call("rabbit_misc:start/1"), 6));
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(found[0].0, Finding::ReturnShapeChanged { .. }));
    assert_eq!(found[0].1, &host_span(&report, &[1]));
}

#[test]
fn a_spec_that_cannot_be_read_is_withheld() {
    let content = spec_host("(term()) -> {ok, pid()}", "an unparsable spec", 1);
    let report = run_one_line(&content, reference_to(call("rabbit_misc:start/1"), 6));
    assert!(report.is_clean());
    assert_eq!(report.tallies().withheld()[&WithheldReason::UnknownSpec], 1);
    let [withheld] = report.withheld() else {
        panic!("one withheld symbol expected");
    };
    assert_eq!(
        withheld.symbol,
        WithheldSymbol::UnknownSpec("rabbit_misc:start/1".parse().unwrap())
    );
    assert_eq!(withheld.span, host_span(&report, &[1]));
}

#[test]
fn a_withheld_symbol_counts_every_site_that_uses_it() {
    let content = spec_host("(term()) -> {ok, pid()}", "an unparsable spec", 1);
    let mut references = ReferenceSet::new();
    for line in [6, 9, 12] {
        references.add(call("rabbit_misc:start/1"), site(line));
    }
    let report = run_one_line(&content, references);
    assert_eq!(report.withheld().len(), 1);
    assert_eq!(report.withheld()[0].sites.as_slice().len(), 3);
    assert_eq!(report.tallies().withheld()[&WithheldReason::UnknownSpec], 3);
}

#[test]
fn an_exported_type_that_is_gone_is_reported() {
    let content = host(|index, builder| {
        let types: &[(&str, u8)] = if index == 0 { &[] } else { &[("state", 0)] };
        builder.module(
            "rabbit",
            with_exported_types(module_with("rabbit_backing_queue", &[]), types),
        )
    });
    let report = run_one_line(
        &content,
        reference_to(
            Reference::Type {
                module: "rabbit_backing_queue".parse().unwrap(),
                name: "state".parse().unwrap(),
                arity: Arity::new(0),
            },
            5,
        ),
    );
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(found[0].0, Finding::TypeMissing { .. }));
    assert_eq!(found[0].1, &host_span(&report, &[0]));
}

#[test]
fn a_partial_and_its_keys_are_compared() {
    let partial = PartialRef {
        app: app("rabbit"),
        name: "ssl_options".parse().unwrap(),
    };
    let content = host(|index, builder| match index {
        0 => builder,
        1 => builder.partial("rabbit", "ssl_options", &["verify"], &[]),
        _ => builder.partial("rabbit", "ssl_options", &["verify", "cacertfile"], &[]),
    });
    let report = run_one_line(
        &content,
        reference_to(
            Reference::Partial {
                partial: partial.clone(),
                keys: ["cacertfile".parse().unwrap()].into(),
            },
            2,
        ),
    );
    let found = findings(&report);
    assert_eq!(found.len(), 2);
    assert!(found.iter().any(|(finding, span)| {
        matches!(finding, Finding::PartialMissing { .. }) && **span == host_span(&report, &[0])
    }));
    assert!(found.iter().any(|(finding, span)| {
        matches!(finding, Finding::PartialKeyMissing { key, .. } if key.as_str() == "cacertfile")
            && **span == host_span(&report, &[1])
    }));
}

#[test]
fn an_otp_export_the_build_patch_lacks_is_a_finding_on_the_older_otp_line() {
    let content = host(|_, builder| builder);
    let otp = [
        ("OTP-27.3", kernel("OTP-27.3")),
        (
            "OTP-28.5",
            kernel("OTP-28.5").module("kernel", module_with("json", &[("encode", 1)])),
        ),
    ];
    let report = run(
        &content,
        &otp,
        &[27, 28],
        reference_to(call("json:encode/1"), 20),
    );
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(matches!(found[0].0, Finding::ModuleMissing { module } if module.as_str() == "json"));
    assert_eq!(found[0].1, &otp_span(&report, &[0]));
    assert_eq!(report.tallies().checked(), 1);
}

#[test]
fn an_app_nothing_provides_is_missing_everywhere() {
    let content = host(|_, builder| builder.app("rabbitmq_amqp_client", &[]));
    let report = run_one_line(
        &content,
        reference_to(Reference::App(app("rabbitmq_amqp_client")), 1),
    );
    let found = findings(&report);
    assert_eq!(found.len(), 1);
    assert!(
        matches!(found[0].0, Finding::AppMissing { app, .. } if app.as_str() == "rabbitmq_amqp_client")
    );
    assert_eq!(found[0].1, &FindingSpan::Everywhere);
    let host_index = report.targets().host_indices().next().unwrap();
    let otp_index = report.targets().otp_indices().next().unwrap();
    assert_eq!(
        report.cell(&app("rabbitmq_shovel_extra"), host_index, otp_index),
        JigsawCell::Broken
    );
}

#[test]
fn a_clean_run_has_every_tally_label_and_clean_cells() {
    let content = host(|_, builder| {
        builder.module("rabbit", module_with("rabbit_amqqueue", &[("lookup", 1)]))
    });
    let report = run_one_line(&content, reference_to(call("rabbit_amqqueue:lookup/1"), 8));
    assert!(report.is_clean());
    assert_eq!(report.tallies().not_checked().len(), NotChecked::ALL.len());
    assert_eq!(report.tallies().withheld().len(), WithheldReason::ALL.len());
    for host_index in report.targets().host_indices() {
        for otp_index in report.targets().otp_indices() {
            assert_eq!(
                report.cell(&app("rabbitmq_shovel_extra"), host_index, otp_index),
                JigsawCell::Clean
            );
        }
    }
}

#[test]
fn a_finding_names_every_site_and_app_that_leads_to_it() {
    let content = host(|index, builder| {
        let exports: &[(&str, u8)] = if index == 0 { &[] } else { &[("lookup", 1)] };
        builder.module("rabbit", module_with("rabbit_amqqueue", exports))
    });
    let mut references = reference_to(call("rabbit_amqqueue:lookup/1"), 8);
    references.add(call("rabbit_amqqueue:lookup/1"), site(15));
    let report = run_one_line(&content, references);
    let ranged = &report.findings()[0];
    assert_eq!(ranged.sites.as_slice().len(), 2);
    assert_eq!(ranged.apps.as_slice(), [app("rabbitmq_shovel_extra")]);
}

#[test]
fn a_macro_whose_string_literal_changed_is_a_warning_not_a_finding() {
    let content = host(|index, builder| {
        let text = if index == 0 {
            "\"2007-2026\""
        } else {
            "\"2007-2025\""
        };
        builder.header(defining(rabbit_hrl(), "COPYRIGHT_MESSAGE", None, text))
    });
    let references = with_host_include(
        reference_to(
            Reference::Macro {
                includer: file("src/rabbit_shovel_extra_worker.erl"),
                key: MacroKey::object_like("COPYRIGHT_MESSAGE".parse().unwrap()),
            },
            5,
        ),
        "src/rabbit_shovel_extra_worker.erl",
    );
    let report = run_one_line(&content, references);
    assert!(report.is_clean());
    assert!(
        report
            .warnings()
            .iter()
            .any(|ranged| matches!(ranged.warning, Warning::MacroTextChanged { .. }))
    );
}
