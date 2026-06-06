// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum SchemaCmd {
    /// Emit the embedded JSON schema for envelope version `<version>`.
    Show(SchemaShowArgs),
    /// Emit a structured diff between two envelope schema versions.
    Diff(SchemaDiffArgs),
    /// Emit the envelope schema versions this binary supports.
    #[command(name = "supported_envelope_versions")]
    SupportedEnvelopeVersions,
}

#[derive(Debug, Args)]
#[command(disable_version_flag = true)]
pub struct SchemaShowArgs {
    /// Envelope schema version to emit.
    pub version: u32,
}

#[derive(Debug, Args)]
pub struct SchemaDiffArgs {
    /// Starting schema version.
    pub from: u32,
    /// Target schema version.
    pub to: u32,
}
