// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::jigsaw::plugin_set::{PluginAppSources, PluginSet, PluginSources, SourceFile};
use backhopper_core::jigsaw::surface::{PinApps, ReleaseSurface, SurfaceSource};
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::names::{ApplicationName, RelativePath};
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_jigsaw::collect;
use backhopper_test_support::{
    SnapshotBuilder, defining, hrl, including, module_with, with_record,
};

const LINES: &[&str] = &[
    "-module(rabbit_shovel_extra_worker).",
    "-include_lib(\"rabbit_common/include/rabbit.hrl\").",
    "-include(\"missing.hrl\").",
    "-behaviour(rabbit_backing_queue).",
    "-ifdef(TEST).",
    "-if(?OTP_RELEASE >= 28).",
    "-else.",
    "-endif.",
    "run(Q) -> rabbit_amqqueue:lookup(Q).",
    "run(#amqqueue{name = N}) -> ?ROUTING_KEY_LIMIT + N.",
    "limit() -> ?RABBIT_LOG(info, \"x ~p\", [self()]).",
    "dyn(M, F) -> M:F(), apply(M, F, []).",
    "% -define(COMMENTED, 1).",
    "-spec fetch(node()) -> rabbit_backing_queue:state().",
    "build(N) -> #amqqueue{name = N, durable = true}.",
    "str() -> \"?NOT_A_MACRO #not_a_record{\".",
    "q() -> 'Elixir.RabbitMQ.CLI':main([]).",
    "    erpc:call(N, rabbit_misc, pget, [a, b]),",
    "-define(LOCAL, 1).",
    "$%.",
    "<<\"unterminated",
];

fn host() -> (Snapshot<state::Canonical>, PinApps) {
    let header = defining(
        with_record(
            including(
                hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                "rabbit_common",
                "include/resource.hrl",
            ),
            "amqqueue",
            &[("name", None), ("durable", Some("false"))],
        ),
        "ROUTING_KEY_LIMIT",
        None,
        "255",
    );
    let snapshot = SnapshotBuilder::new("rabbitmq-server", "v4.2.8")
        .roots(&["rabbit"])
        .app("rabbit", &["rabbit_common"])
        .app("rabbit_common", &[])
        .module("rabbit", module_with("rabbit_amqqueue", &[("lookup", 1)]))
        .header(header)
        .build();
    let apps = PinApps::of(&snapshot).unwrap();
    (snapshot, apps)
}

fn plugin(text: String) -> PluginSet {
    let name = ApplicationName::new("rabbitmq_shovel_extra").unwrap();
    PluginSet::new(PluginSources {
        apps: vec![PluginAppSources {
            root: PathBuf::from("/plugins/rabbitmq_shovel_extra"),
            manifest: AppManifest {
                name,
                source: ManifestSource::AppSrc,
                requires: ManifestDeps::Listed(BTreeSet::new()),
            }
            .into_read()
            .unwrap(),
            manifest_path: RelativePath::new("src/rabbitmq_shovel_extra.app.src").unwrap(),
            files: vec![SourceFile {
                path: RelativePath::new("src/rabbit_shovel_extra_worker.erl").unwrap(),
                bytes: text.into_bytes(),
            }],
        }],
        host_provided: BTreeSet::new(),
    })
    .unwrap()
}

proptest! {
    #[test]
    fn any_mix_of_erlang_lines_reads_and_every_site_is_inside_the_file(
        picks in prop::collection::vec(0..LINES.len(), 1..40),
    ) {
        let text = picks.iter().map(|&i| LINES[i]).collect::<Vec<_>>().join("\n");
        let line_count = u32::try_from(text.lines().count()).unwrap();
        let (snapshot, apps) = host();
        let untracked = BTreeSet::new();
        let roots = [ApplicationName::new("rabbit").unwrap()];
        let build = ReleaseSurface::new(
            &[SurfaceSource { snapshot: &snapshot, apps: &apps }],
            &roots,
            &untracked,
        );
        let references = collect(&plugin(text), &build).unwrap();
        for sites in references.external().values() {
            for site in sites {
                prop_assert!(site.line >= 1);
                prop_assert!(site.line <= line_count.max(1), "{} beyond {line_count}", site.line);
            }
        }
    }
}
