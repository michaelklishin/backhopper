// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The plugin and host used across the reference tests.

use backhopper_test_support::{
    SnapshotBuilder, defining, defining_conditional, hrl, including, module_with, with_callbacks,
    with_exported_types, with_record, with_spec,
};

use crate::support::Build;

pub(crate) const WORKER: &str = "src/rabbit_shovel_extra_worker.erl";

pub(crate) fn lines(lines: &[&str]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// A host with the pieces a shovel-style plugin builds on.
pub(crate) fn rabbit_host() -> Build {
    let resource = with_record(
        hrl("rabbit_common", "deps/rabbit_common/include/resource.hrl"),
        "resource",
        &[("virtual_host", None), ("kind", None), ("name", None)],
    );
    let rabbit_hrl = defining(
        defining(
            including(
                hrl("rabbit_common", "deps/rabbit_common/include/rabbit.hrl"),
                "rabbit_common",
                "include/resource.hrl",
            ),
            "ROUTING_KEY_LIMIT",
            None,
            "255",
        ),
        "RABBIT_LOG",
        Some(2),
        "rabbit_log:log(A1, A2)",
    );
    let rabbit_hrl = with_record(
        rabbit_hrl,
        "amqqueue",
        &[("name", None), ("durable", Some("false")), ("pid", None)],
    );
    let snapshot = SnapshotBuilder::new("rabbitmq-server", "v4.2.8")
        .roots(&["rabbit"])
        .app("rabbit", &["rabbit_common", "ra"])
        .app("rabbit_common", &[])
        .app("ra", &[])
        .module(
            "rabbit",
            module_with(
                "rabbit_amqqueue",
                &[("lookup", 1), ("lookup_many", 2), ("declare", 4)],
            ),
        )
        .module(
            "rabbit_common",
            with_spec(
                module_with("rabbit_misc", &[("pget", 2), ("rpc_call", 4), ("pget", 3)]),
                "pget",
                2,
                "(term(), list()) -> {ok, pid()}",
            ),
        )
        .module("rabbit", module_with("rabbit_log", &[("log", 2)]))
        .module(
            "rabbit",
            with_callbacks(
                with_exported_types(module_with("rabbit_backing_queue", &[]), &[("state", 0)]),
                &[("init", 3), ("terminate", 2)],
                &[("terminate", 2)],
            ),
        )
        .header(rabbit_hrl)
        .header(resource)
        .header(defining_conditional(
            hrl(
                "rabbit_common",
                "deps/rabbit_common/include/rabbit_framing.hrl",
            ),
            "FRAME_MAX",
            None,
        ))
        .partial("rabbit", "ssl_options", &["cacertfile", "verify"], &[])
        .schema("rabbit", "rabbit", &["listeners.tcp"], &["port"])
        .build();
    Build::new(snapshot, &["rabbit"])
}
