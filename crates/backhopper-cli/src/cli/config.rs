// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Print the loaded config as canonical TOML.
    Show,
    /// Parse and validate the config; exit non-zero if invalid.
    Validate,
}
