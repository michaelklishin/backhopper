// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Build-system detection: looks at the repo root to figure out
//! which Erlang build tool is in use, so we can emit a footer hint
//! after suite-selection output.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildSystem {
    Rebar3,
    ErlangMk,
    Unknown,
}

impl BuildSystem {
    /// Detects which Erlang build tool the repo uses. The heuristic:
    ///  * `rebar.config` (or `rebar.config.script`) present → rebar3
    ///  * `erlang.mk` present, OR a top-level `Makefile` includes it → erlang.mk
    ///  * otherwise → unknown
    ///
    /// rebar3 wins ties because RabbitMQ-style umbrella projects also
    /// ship an erlang.mk, but per-app `rebar.config` is the more
    /// authoritative declaration for the suite-running command.
    pub fn detect(repo_root: &Path) -> Self {
        if repo_root.join("rebar.config").is_file()
            || repo_root.join("rebar.config.script").is_file()
        {
            return Self::Rebar3;
        }
        if repo_root.join("erlang.mk").is_file() {
            return Self::ErlangMk;
        }
        if let Ok(mk) = fs::read_to_string(repo_root.join("Makefile"))
            && (mk.contains("erlang.mk") || mk.contains("PROJECT ="))
        {
            return Self::ErlangMk;
        }
        Self::Unknown
    }
}
