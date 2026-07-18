// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Hardcoded allowlist of common OTP module names.
//!
//! Used to label untracked-call diagnostics so operators can tell at a
//! glance which untracked calls are routine OTP usage versus a missing
//! dependency in `backhopper.toml`. Not exhaustive: limited to modules
//! we observe regularly in RabbitMQ source. Extend as needed.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::model::names::ModuleName;

pub fn is_otp_module(module: &ModuleName) -> bool {
    set().contains(module.as_str())
}

fn set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| OTP_MODULES.iter().copied().collect())
}

const OTP_MODULES: &[&str] = &[
    "application",
    "array",
    "asn1rt",
    "base64",
    "beam_lib",
    "binary",
    "calendar",
    "code",
    "compile",
    "crypto",
    "dets",
    "dict",
    "digraph",
    "digraph_utils",
    "edoc",
    "epp",
    "erl_eval",
    "erl_parse",
    "erl_prim_loader",
    "erl_scan",
    "erlang",
    "erpc",
    "error_handler",
    "error_logger",
    "escript",
    "ets",
    "eunit",
    "file",
    "filelib",
    "filename",
    "gb_sets",
    "gb_trees",
    "gen_event",
    "gen_fsm",
    "gen_sctp",
    "gen_server",
    "gen_statem",
    "gen_tcp",
    "gen_udp",
    "global",
    "httpc",
    "httpd",
    "inet",
    "inet_db",
    "inet_res",
    "inets",
    "io",
    "io_lib",
    "lists",
    "logger",
    "maps",
    "math",
    "mnesia",
    "net",
    "net_adm",
    "net_kernel",
    "orddict",
    "ordsets",
    "os",
    "pg",
    "pg2",
    "proc_lib",
    "proplists",
    "public_key",
    "queue",
    "rand",
    "random",
    "re",
    "rpc",
    "sasl",
    "seq_trace",
    "sets",
    "shell",
    "ssh",
    "ssl",
    "string",
    "supervisor",
    "supervisor_bridge",
    "sys",
    "systools",
    "timer",
    "unicode",
    "uri_string",
    "zip",
    "zlib",
];
