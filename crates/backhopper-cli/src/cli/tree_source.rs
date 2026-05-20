// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct TreeSource {
    /// Directory containing the Erlang source tree to analyse.
    #[arg(long)]
    pub tree_dir_path: PathBuf,
}