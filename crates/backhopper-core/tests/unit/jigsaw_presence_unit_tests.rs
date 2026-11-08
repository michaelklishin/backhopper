// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::path::PathBuf;

use backhopper_core::jigsaw::evaluate;
use backhopper_core::jigsaw::findings::{Finding, Stop, Warning, WithheldReason, WithheldSymbol};
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginFile, PluginSet, PluginSources};
use backhopper_core::jigsaw::references::{
    NotChecked, Reference, ReferenceSet, ReferenceSite, SiteDetail,
};
use backhopper_core::jigsaw::report::FindingSpan;
use backhopper_core::jigsaw::report::JigsawReport;
use backhopper_core::jigsaw::surface::{
    AppPresence, OtpAppPresence, OtpSurface, PinApps, ReleaseSurface, SurfaceSource,
};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::macro_decl::MacroKey;
use backhopper_core::model::names::{ApplicationName, DependencyName, RelativePath};
use backhopper_core::model::snapshot::HeaderRef;
use backhopper_test_support::{
    SnapshotBuilder, defining, host_config_excluding_otp_apps, hrl, loaded_plan,
    loaded_plan_with_config, module_with,
};

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn presence(
    snapshot: &SnapshotBuilder,
    roots: &[&str],
    untracked: &[&str],
    asked: &str,
) -> AppPresence {
    let snapshot = snapshot.clone().build();
    let apps = PinApps::of(&snapshot).unwrap();
    let untracked: BTreeSet<DependencyName> = untracked
        .iter()
        .map(|name| DependencyName::new(*name).unwrap())
        .collect();
    let roots: Vec<ApplicationName> = roots.iter().map(|name| app(name)).collect();
    let surface = ReleaseSurface::new(
        &[SurfaceSource {
            snapshot: &snapshot,
            apps: &apps,
        }],
        &roots,
        &untracked,
    );
    surface.shipped().lookup(&app(asked))
}

fn tree() -> SnapshotBuilder {
    SnapshotBuilder::new("rabbitmq-server", "v4.2.3")
        .app("rabbit", &["rabbit_common", "ra"])
        .app("rabbit_common", &["thoas"])
        .app("thoas", &[])
        .app("ra", &[])
        .app("rabbitmq_amqp_client", &[])
}

#[test]
fn an_app_reachable_from_the_roots_through_two_manifests_is_shipped() {
    assert_eq!(
        presence(&tree(), &["rabbit"], &[], "thoas"),
        AppPresence::Provided
    );
}

#[test]
fn an_app_only_in_the_tree_is_not_shipped() {
    assert_eq!(
        presence(&tree(), &["rabbit"], &[], "rabbitmq_amqp_client"),
        AppPresence::Absent
    );
}

#[test]
fn an_unreadable_manifest_withholds_what_is_beyond_it() {
    let snapshot = SnapshotBuilder::new("rabbitmq-server", "v4.2.3")
        .unreadable_app("rabbit")
        .app("rabbitmq_amqp_client", &[]);
    assert_eq!(
        presence(&snapshot, &["rabbit"], &[], "rabbitmq_amqp_client"),
        AppPresence::Stopped(Stop::UnreadableManifest)
    );
    assert_eq!(
        presence(&snapshot, &["rabbit"], &[], "rabbit"),
        AppPresence::Provided
    );
}

#[test]
fn an_untracked_dependency_stops_the_closure_and_is_withheld() {
    let snapshot = SnapshotBuilder::new("rabbitmq-server", "v4.2.3").app("rabbit", &["ranch"]);
    assert_eq!(
        presence(&snapshot, &["rabbit"], &["ranch"], "ranch"),
        AppPresence::Provided
    );
    assert_eq!(
        presence(&snapshot, &["rabbit"], &["ranch"], "cowlib"),
        AppPresence::Stopped(Stop::UntrackedApp)
    );
    assert_eq!(
        presence(&snapshot, &["rabbit"], &[], "cowlib"),
        AppPresence::Absent
    );
}

#[test]
fn an_excluded_otp_app_is_withheld_not_absent() {
    let snapshot = SnapshotBuilder::new("otp", "OTP-27.3")
        .app("kernel", &[])
        .build();
    let excluded = BTreeSet::from([app("ssh")]);
    let surface = OtpSurface::new(&snapshot, &excluded);
    assert_eq!(surface.app(&app("kernel")), OtpAppPresence::Provided);
    assert_eq!(surface.app(&app("ssh")), OtpAppPresence::Excluded);
    assert_eq!(surface.app(&app("wx")), OtpAppPresence::Absent);
}

fn plugin_with(apps: &[(&str, &[&str])]) -> PluginSet {
    PluginSet::new(PluginSources {
        apps: apps
            .iter()
            .map(|(name, requires)| PluginAppSources {
                root: PathBuf::from(format!("/plugins/{name}")),
                manifest: AppManifest {
                    name: app(name),
                    source: ManifestSource::AppSrc,
                    requires: ManifestDeps::Listed(requires.iter().map(|r| app(r)).collect()),
                }
                .into_read()
                .unwrap(),
                manifest_path: RelativePath::new(format!("src/{name}.app.src")).unwrap(),
                files: Vec::new(),
            })
            .collect(),
        host_provided: BTreeSet::new(),
    })
    .unwrap()
}

fn site(app_name: &str) -> ReferenceSite {
    ReferenceSite {
        file: PluginFile {
            app: app(app_name),
            path: RelativePath::new("src/worker.erl").unwrap(),
        },
        line: 4,
        detail: SiteDetail::Plain,
    }
}

fn kernel(tag: &str) -> SnapshotBuilder {
    SnapshotBuilder::new("otp", tag).app("kernel", &[])
}

fn run(
    plugins: &PluginSet,
    host_content: impl Fn(SnapshotBuilder) -> SnapshotBuilder,
    references: ReferenceSet,
) -> JigsawReport {
    let host: Vec<(&str, SnapshotBuilder)> = ["v4.2.0", "v4.2.1"]
        .into_iter()
        .map(|tag| {
            let base = SnapshotBuilder::new("rabbitmq-server", tag)
                .roots(&["rabbit"])
                .app("rabbit", &[]);
            (tag, host_content(base))
        })
        .collect();
    let loaded = loaded_plan(&host, &[("OTP-27.3", kernel("OTP-27.3"))], &[27]);
    evaluate(&loaded, plugins, |_, _| Ok::<_, Infallible>(references)).unwrap()
}

#[test]
fn a_call_into_an_in_tree_app_the_release_does_not_ship_is_a_missing_app() {
    let plugins = plugin_with(&[("rabbitmq_shovel_extra", &[])]);
    let mut references = ReferenceSet::new();
    references.add(
        Reference::Call("rabbitmq_amqp_client:connect/1".parse().unwrap()),
        site("rabbitmq_shovel_extra"),
    );
    let report = run(
        &plugins,
        |base| {
            base.app("rabbitmq_amqp_client", &[]).module(
                "rabbitmq_amqp_client",
                module_with("rabbitmq_amqp_client", &[("connect", 1)]),
            )
        },
        references,
    );
    assert_eq!(report.findings().len(), 1);
    assert!(matches!(
        &report.findings()[0].finding,
        Finding::AppMissing { app: missing, required_by }
            if missing.as_str() == "rabbitmq_amqp_client"
                && required_by.as_slice() == [app("rabbitmq_shovel_extra")]
    ));
}

#[test]
fn a_bundled_app_is_never_missing() {
    let plugins = plugin_with(&[
        ("rabbitmq_shovel_extra", &["rabbitmq_amqp_client"]),
        ("rabbitmq_amqp_client", &[]),
    ]);
    let mut references = ReferenceSet::new();
    references.add(
        Reference::App(app("rabbitmq_amqp_client")),
        site("rabbitmq_shovel_extra"),
    );
    let report = run(
        &plugins,
        |base| base.app("rabbitmq_amqp_client", &[]),
        references,
    );
    assert!(report.is_clean());
    assert_eq!(report.apps().len(), 2);
}

#[test]
fn a_validator_only_a_non_shipped_app_defines_is_not_compared() {
    let plugins = plugin_with(&[("rabbitmq_shovel_extra", &[])]);
    let mut references = ReferenceSet::new();
    references.add(
        Reference::Validator("port".parse().unwrap()),
        site("rabbitmq_shovel_extra"),
    );
    let report = run(
        &plugins,
        |base| {
            base.app("rabbitmq_amqp_client", &[]).schema(
                "rabbitmq_amqp_client",
                "amqp_client",
                &[],
                &["port"],
            )
        },
        references,
    );
    assert!(report.is_clean());
    assert_eq!(
        report.tallies().not_checked()[&NotChecked::UnresolvedAtBuild],
        1
    );
}

#[test]
fn an_app_bundled_and_shipped_is_a_warning_that_does_not_fail_the_run() {
    let plugins = plugin_with(&[("rabbit", &[])]);
    let report = run(&plugins, |base| base, ReferenceSet::new());
    assert!(report.is_clean());
    assert_eq!(report.warnings().len(), 1);
    assert_eq!(report.warnings()[0].warning.symbol(), "rabbit");
}

#[test]
fn an_otp_app_the_otp_tag_provides_is_present() {
    let plugins = plugin_with(&[("rabbitmq_shovel_extra", &["kernel"])]);
    let mut references = ReferenceSet::new();
    references.add(Reference::App(app("kernel")), site("rabbitmq_shovel_extra"));
    let report = run(&plugins, |base| base, references);
    assert!(report.is_clean());
    assert_eq!(report.tallies().checked(), 1);
}

#[test]
fn an_otp_app_the_snapshot_excludes_is_counted_not_missing() {
    let plugins = plugin_with(&[("rabbitmq_shovel_extra", &["ssh"])]);
    let mut references = ReferenceSet::new();
    references.add(Reference::App(app("ssh")), site("rabbitmq_shovel_extra"));
    let host = [(
        "v4.2.0",
        SnapshotBuilder::new("rabbitmq-server", "v4.2.0")
            .roots(&["rabbit"])
            .app("rabbit", &[]),
    )];
    let otp = [("OTP-27.3", kernel("OTP-27.3"))];
    let loaded = loaded_plan_with_config(
        &host_config_excluding_otp_apps(&["ssh"]),
        &host,
        &otp,
        &[27],
    );
    let report = evaluate(&loaded, &plugins, |_, _| Ok::<_, Infallible>(references)).unwrap();
    assert!(report.is_clean());
    assert_eq!(
        report.tallies().withheld()[&WithheldReason::ExcludedOtpApp],
        1
    );
    let [withheld] = report.withheld() else {
        panic!("one withheld symbol expected");
    };
    assert_eq!(withheld.symbol, WithheldSymbol::ExcludedOtpApp(app("ssh")));
    assert!(matches!(withheld.span, FindingSpan::Otp(_)));
}

#[test]
fn an_app_the_build_patch_cannot_place_is_withheld_at_every_release() {
    let plugins = plugin_with(&[("rabbitmq_shovel_extra", &[])]);
    let mut references = ReferenceSet::new();
    references.add(
        Reference::App(app("rabbitmq_amqp_client")),
        site("rabbitmq_shovel_extra"),
    );
    let host: Vec<(&str, SnapshotBuilder)> = ["v4.2.0", "v4.2.1"]
        .into_iter()
        .map(|tag| {
            let content = SnapshotBuilder::new("rabbitmq-server", tag)
                .roots(&["rabbit"])
                .unreadable_app("rabbit")
                .app("rabbitmq_amqp_client", &[]);
            (tag, content)
        })
        .collect();
    let loaded = loaded_plan(&host, &[("OTP-27.3", kernel("OTP-27.3"))], &[27]);
    let report = evaluate(&loaded, &plugins, |_, _| Ok::<_, Infallible>(references)).unwrap();
    assert!(report.is_clean());
    assert_eq!(
        report.tallies().withheld()[&WithheldReason::UnreadableManifest],
        1
    );
    assert!(report.withheld().is_empty());
    let warned: Vec<_> = report
        .warnings()
        .iter()
        .filter(|ranged| matches!(&ranged.warning, Warning::ManifestUnreadable { .. }))
        .map(|ranged| ranged.warning.symbol())
        .collect();
    assert_eq!(warned, vec!["rabbit".to_owned()]);
}

#[test]
fn two_references_that_ask_the_same_question_are_one_checked_anchor() {
    let plugins = plugin_with(&[("rabbitmq_shovel_extra", &[])]);
    let mut references = ReferenceSet::new();
    let includers = ["src/a.erl", "src/b.erl"];
    for includer in includers {
        let file = PluginFile {
            app: app("rabbitmq_shovel_extra"),
            path: RelativePath::new(includer).unwrap(),
        };
        references.set_host_includes(
            file.clone(),
            BTreeSet::from([HeaderRef {
                app: app("rabbit_common"),
                app_path: RelativePath::new("include/rabbit.hrl").unwrap(),
            }]),
        );
        references.add(
            Reference::Macro {
                includer: file.clone(),
                key: MacroKey::object_like("ROUTING_KEY_LIMIT".parse().unwrap()),
            },
            ReferenceSite {
                file: PluginFile {
                    app: app("rabbitmq_shovel_extra"),
                    path: RelativePath::new("include/limits.hrl").unwrap(),
                },
                line: 2,
                detail: SiteDetail::Plain,
            },
        );
    }
    let host: Vec<(&str, SnapshotBuilder)> = ["v4.2.0", "v4.2.1"]
        .into_iter()
        .map(|tag| {
            let content = SnapshotBuilder::new("rabbitmq-server", tag)
                .roots(&["rabbit"])
                .app("rabbit", &["rabbit_common"])
                .app("rabbit_common", &[])
                .header(defining(
                    hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                    "ROUTING_KEY_LIMIT",
                    None,
                    "255",
                ));
            (tag, content)
        })
        .collect();
    let loaded = loaded_plan(&host, &[("OTP-27.3", kernel("OTP-27.3"))], &[27]);
    let report = evaluate(&loaded, &plugins, |_, _| Ok::<_, Infallible>(references)).unwrap();
    assert!(report.is_clean());
    assert_eq!(report.tallies().checked(), 2, "the macro and its owner app");
}
