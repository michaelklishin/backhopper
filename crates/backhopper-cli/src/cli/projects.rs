// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::Subcommand;

use backhopper_core::model::names::ProjectName;

#[derive(Debug, Subcommand)]
pub enum ProjectsCmd {
    /// List configured projects.
    List,
    /// Show a single project's configuration.
    Show {
        #[arg(long)]
        project: ProjectName,
    },
}