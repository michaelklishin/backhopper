// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::path::PathBuf;

use backhopper_core::jigsaw::evaluate;
use backhopper_core::jigsaw::findings::Warning;
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginFile, PluginSet, PluginSources};
use backhopper_core::jigsaw::references::{
    RecordConstruction, Reference, ReferenceSet, ReferenceSite, SiteDetail,
};
use backhopper_core::jigsaw::report::{FindingSpan, JigsawReport};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::names::{ApplicationName, FieldName, RelativePath};
use backhopper_core::model::snapshot::HeaderRef;
use backhopper_test_support::{SnapshotBuilder, hrl, loaded_plan, with_record};

const RELEASES: [&str; 3] = ["v4.2.0", "v4.2.1", "v4.2.2"];

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn plugin() -> PluginSet {
    PluginSet::new(PluginSources {
        apps: vec![PluginAppSources {
            root: PathBuf::from("/plugins/rabbitmq_shovel_extra"),
            manifest: AppManifest {
                name: app("rabbitmq_shovel_extra"),
                source: ManifestSource::AppSrc,
                requires: ManifestDeps::Listed(BTreeSet::new()),
            }
            .into_read()
            .unwrap(),
            manifest_path: RelativePath::new("src/rabbitmq_shovel_extra.app.src").unwrap(),
            files: Vec::new(),
        }],
        host_provided: BTreeSet::new(),
    })
    .unwrap()
}

fn construction(unset: &[&str]) -> ReferenceSet {
    let includer = PluginFile {
        app: app("rabbitmq_shovel_extra"),
        path: RelativePath::new("src/rabbit_shovel_extra_worker.erl").unwrap(),
    };
    let mut references = ReferenceSet::new();
    references.add(
        Reference::Record {
            includer: includer.clone(),
            name: "amqqueue".parse().unwrap(),
        },
        ReferenceSite {
            file: includer.clone(),
            line: 21,
            detail: SiteDetail::Construction(RecordConstruction {
                unset: unset.iter().map(|f| FieldName::new(*f).unwrap()).collect(),
            }),
        },
    );
    references.set_host_includes(
        includer,
        BTreeSet::from([HeaderRef {
            app: app("rabbit_common"),
            app_path: RelativePath::new("include/rabbit.hrl").unwrap(),
        }]),
    );
    references
}

/// `durable` defaults to `defaults[i]` at release `i`; the build patch is the last.
fn run(defaults: [Option<&str>; 3], references: ReferenceSet) -> JigsawReport {
    let host: Vec<(&str, SnapshotBuilder)> = RELEASES
        .iter()
        .zip(defaults)
        .map(|(tag, default)| {
            let content = SnapshotBuilder::new("rabbitmq-server", tag)
                .roots(&["rabbit"])
                .app("rabbit", &["rabbit_common"])
                .app("rabbit_common", &[])
                .header(with_record(
                    hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                    "amqqueue",
                    &[("name", None), ("durable", default)],
                ));
            (*tag, content)
        })
        .collect();
    let otp = [(
        "OTP-27.3",
        SnapshotBuilder::new("otp", "OTP-27.3").app("kernel", &[]),
    )];
    let loaded = loaded_plan(&host, &otp, &[27]);
    evaluate(&loaded, &plugin(), |_, _| Ok::<_, Infallible>(references)).unwrap()
}

#[test]
fn a_default_that_changed_is_a_warning_when_a_construction_leaves_the_field_unset() {
    let report = run(
        [Some("true"), Some("false"), Some("false")],
        construction(&["durable"]),
    );
    assert_eq!(report.warnings().len(), 1);
    let ranged = &report.warnings()[0];
    assert!(matches!(
        &ranged.warning,
        Warning::RecordDefaultChanged { field, build, target, .. }
            if field.as_str() == "durable"
                && build.as_ref().map(|d| d.as_str()) == Some("false")
                && target.as_ref().map(|d| d.as_str()) == Some("true")
    ));
    let FindingSpan::Host(releases) = &ranged.span else {
        panic!("a default is compared per host release");
    };
    assert_eq!(releases.len(), 1);
}

#[test]
fn a_field_every_construction_sets_gives_no_warning() {
    let report = run(
        [Some("true"), Some("false"), Some("false")],
        construction(&[]),
    );
    assert!(report.warnings().is_empty());
}

#[test]
fn no_default_and_an_explicit_undefined_compare_equal() {
    let report = run([Some("undefined"), None, None], construction(&["durable"]));
    assert!(report.warnings().is_empty());
}

#[test]
fn a_warning_does_not_change_whether_the_run_is_clean() {
    let report = run(
        [Some("true"), Some("false"), Some("false")],
        construction(&["durable"]),
    );
    assert!(report.is_clean());
    assert!(report.findings().is_empty());
}
