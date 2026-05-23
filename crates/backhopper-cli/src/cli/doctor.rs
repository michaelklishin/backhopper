// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::Args;

/// Workspace health summary. One row per series-pin, showing whether the
/// pinned tag has a snapshot on disk plus optional upstream-freshness info.
#[derive(Debug, Args, Clone)]
pub struct DoctorCmd {
    /// Talk to each project's remote (`ls-remote`) and report how many tags
    /// upstream has past the latest one we know locally. Costs one network
    /// roundtrip per project, so off by default.
    #[arg(long)]
    pub check_remote: bool,

    /// Skip series whose name does not match this substring.
    #[arg(long)]
    pub series: Option<String>,
}
