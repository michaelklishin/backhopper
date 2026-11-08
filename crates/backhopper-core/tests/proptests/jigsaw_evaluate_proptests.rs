// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::jigsaw::LoadedPlan;
use backhopper_core::jigsaw::evaluate;
use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginFile, PluginSet, PluginSources};
use backhopper_core::jigsaw::references::{Reference, ReferenceSet, ReferenceSite, SiteDetail};
use backhopper_core::jigsaw::report::{HostIdx, JigsawReport, TargetAxes};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::macro_decl::MacroKey;
use backhopper_core::model::names::{ApplicationName, RelativePath};
use backhopper_core::model::schema_files::PartialRef;
use backhopper_core::model::snapshot::HeaderRef;
use backhopper_test_support::{
    SnapshotBuilder, defining, hrl, loaded_plan, module_with, with_record,
};

use crate::jigsaw_direct_evaluator::{direct, expanded};

const FUNCTIONS: [&str; 4] = ["lookup", "declare", "delete", "count"];
const OTP_FUNCTIONS: [&str; 3] = ["keyfind", "merge", "split"];

#[derive(Debug, Clone, Copy)]
struct ReleaseShape {
    functions: [bool; 4],
    record: u8,
    macro_body: u8,
    partial: Option<u8>,
}

fn release_shape() -> impl Strategy<Value = ReleaseShape> {
    (any::<[bool; 4]>(), 0u8..3, 0u8..3, prop::option::of(0u8..4)).prop_map(
        |(functions, record, macro_body, partial)| ReleaseShape {
            functions,
            record,
            macro_body,
            partial,
        },
    )
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

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn worker() -> PluginFile {
    PluginFile {
        app: app("rabbitmq_shovel_extra"),
        path: RelativePath::new("src/rabbit_shovel_extra_worker.erl").unwrap(),
    }
}

fn site(line: u32) -> ReferenceSite {
    ReferenceSite {
        file: worker(),
        line,
        detail: SiteDetail::Plain,
    }
}

fn host_content(tag: &str, shape: ReleaseShape) -> SnapshotBuilder {
    let exports: Vec<(&str, u8)> = FUNCTIONS
        .iter()
        .zip(shape.functions)
        .filter(|(_, present)| *present)
        .map(|(name, _)| (*name, 1))
        .collect();
    let fields: &[(&str, Option<&str>)] = match shape.record {
        0 => &[("name", None), ("durable", Some("false"))],
        1 => &[("name", None), ("durable", Some("false")), ("pid", None)],
        _ => &[("durable", Some("false")), ("name", None)],
    };
    let body = ["255", "256", "257"][usize::from(shape.macro_body)];
    let header = defining(
        with_record(
            hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
            "amqqueue",
            fields,
        ),
        "ROUTING_KEY_LIMIT",
        None,
        body,
    );
    let mut builder = SnapshotBuilder::new("rabbitmq-server", tag)
        .roots(&["rabbit"])
        .app("rabbit", &["rabbit_common"])
        .app("rabbit_common", &[])
        .module("rabbit", module_with("rabbit_amqqueue", &exports))
        .header(header);
    if let Some(keys) = shape.partial {
        let all = ["cacertfile", "verify"];
        let present: Vec<&str> = all
            .iter()
            .enumerate()
            .filter(|(index, _)| keys & (1 << index) != 0)
            .map(|(_, key)| *key)
            .collect();
        builder = builder.partial("rabbit", "ssl_options", &present, &[]);
    }
    builder
}

fn otp_content(tag: &str, functions: [bool; 3]) -> SnapshotBuilder {
    let exports: Vec<(&str, u8)> = OTP_FUNCTIONS
        .iter()
        .zip(functions)
        .filter(|(_, present)| *present)
        .map(|(name, _)| (*name, 1))
        .collect();
    SnapshotBuilder::new("otp", tag)
        .app("kernel", &[])
        .module("kernel", module_with("lists", &exports))
}

const RELEASES: [&str; 5] = ["v4.2.0", "v4.2.1", "v4.2.2", "v4.2.3", "v4.2.4"];
const OTP_TAGS: [&str; 3] = ["OTP-27.3", "OTP-27.3.1", "OTP-27.3.2"];

fn plan_for(shapes: &[ReleaseShape], otp: &[[bool; 3]]) -> LoadedPlan {
    let host: Vec<(&str, SnapshotBuilder)> = RELEASES
        .iter()
        .zip(shapes)
        .map(|(tag, shape)| (*tag, host_content(tag, *shape)))
        .collect();
    let otp: Vec<(&str, SnapshotBuilder)> = OTP_TAGS
        .iter()
        .zip(otp)
        .map(|(tag, functions)| (*tag, otp_content(tag, *functions)))
        .collect();
    loaded_plan(&host, &otp, &[27])
}

fn references(host_calls: [bool; 4], otp_calls: [bool; 3]) -> ReferenceSet {
    let mut set = ReferenceSet::new();
    let mut line = 1;
    for (name, wanted) in FUNCTIONS.iter().zip(host_calls) {
        if wanted {
            set.add(
                Reference::Call(format!("rabbit_amqqueue:{name}/1").parse().unwrap()),
                site(line),
            );
            line += 1;
        }
    }
    for (name, wanted) in OTP_FUNCTIONS.iter().zip(otp_calls) {
        if wanted {
            set.add(
                Reference::Call(format!("lists:{name}/1").parse().unwrap()),
                site(line),
            );
            line += 1;
        }
    }
    set.add(
        Reference::Record {
            includer: worker(),
            name: "amqqueue".parse().unwrap(),
        },
        site(40),
    );
    set.add(
        Reference::Macro {
            includer: worker(),
            key: MacroKey::object_like("ROUTING_KEY_LIMIT".parse().unwrap()),
        },
        site(41),
    );
    set.add(
        Reference::Partial {
            partial: PartialRef {
                app: app("rabbit"),
                name: "ssl_options".parse().unwrap(),
            },
            keys: ["cacertfile".parse().unwrap()].into(),
        },
        site(42),
    );
    set.set_host_includes(
        worker(),
        BTreeSet::from([HeaderRef {
            app: app("rabbit_common"),
            app_path: RelativePath::new("include/rabbit.hrl").unwrap(),
        }]),
    );
    set
}

fn report(loaded: &LoadedPlan, references: ReferenceSet) -> JigsawReport {
    evaluate(loaded, &plugin(), |_, _| Ok::<_, Infallible>(references)).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn per_surface_passes_give_the_findings_of_the_direct_evaluator(
        shapes in prop::collection::vec(release_shape(), 5),
        otp in prop::collection::vec(any::<[bool; 3]>(), 3),
        host_calls in any::<[bool; 4]>(),
        otp_calls in any::<[bool; 3]>(),
    ) {
        let loaded = plan_for(&shapes, &otp);
        let set = references(host_calls, otp_calls);
        let report = report(&loaded, set.clone());
        let expected = direct(&loaded, &plugin(), &set);
        prop_assert_eq!(expanded(&report), expected);
    }

    #[test]
    fn a_reference_the_build_patch_does_not_satisfy_never_changes_the_findings(
        shapes in prop::collection::vec(release_shape(), 5),
        otp in prop::collection::vec(any::<[bool; 3]>(), 3),
        host_calls in any::<[bool; 4]>(),
    ) {
        let loaded = plan_for(&shapes, &otp);
        let plain = report(&loaded, references(host_calls, [false; 3]));
        let mut with_unknown = references(host_calls, [false; 3]);
        with_unknown.add(
            Reference::Call("rabbit_amqqueue:never_existed/7".parse().unwrap()),
            site(90),
        );
        let extended = report(&loaded, with_unknown);
        prop_assert_eq!(plain.findings(), extended.findings());
        prop_assert_eq!(plain.is_clean(), extended.is_clean());
    }

    #[test]
    fn the_runs_of_any_subset_of_releases_cover_exactly_that_subset(
        members in prop::collection::btree_set(0usize..5, 0..6),
    ) {
        let loaded = plan_for(&[ReleaseShape { functions: [true; 4], record: 0, macro_body: 0, partial: None }; 5], &[[true; 3]; 3]);
        let report = report(&loaded, ReferenceSet::new());
        let indices: Vec<HostIdx> = report.targets().host_indices().collect();
        let set: BTreeSet<HostIdx> = members.iter().map(|&p| indices[p]).collect();
        let runs = TargetAxes::runs(&set);
        let covered: BTreeSet<HostIdx> = runs
            .iter()
            .flat_map(|run| indices.iter().copied().filter(move |i| *i >= run.first && *i <= run.last))
            .collect();
        prop_assert_eq!(&covered, &set);
        for pair in runs.windows(2) {
            let gap = indices.iter().filter(|i| **i > pair[0].last && **i < pair[1].first).count();
            prop_assert!(gap >= 1, "runs {:?} and {:?} are adjacent", pair[0], pair[1]);
        }
    }
}
