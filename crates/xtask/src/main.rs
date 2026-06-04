// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Workspace helper tasks.
//!
//! Today this crate ships one verb: `gen-schema`, which generates the
//! JSON-schema document(s) used by `backhopper schema show <N>` and
//! verifies that on-disk schema files match the current types.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use xtask::{CURRENT_SCHEMA_VERSION, rendered_schema};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Workspace helper tasks for backhopper")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate the JSON schema for the current envelope shape and
    /// either verify on-disk files match (default) or overwrite them
    /// (`--bless`).
    GenSchema {
        /// When set, overwrite the on-disk schema files instead of
        /// comparing. CI runs without this flag.
        #[arg(long)]
        bless: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::GenSchema { bless } => match run_gen_schema(bless) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask gen-schema: {e:?}");
                ExitCode::from(1)
            }
        },
    }
}

fn run_gen_schema(bless: bool) -> Result<()> {
    let schema_dir = workspace_schema_dir();
    fs::create_dir_all(&schema_dir)
        .with_context(|| format!("create schema dir {}", schema_dir.display()))?;

    for version in 1..=CURRENT_SCHEMA_VERSION {
        let target = schema_dir.join(format!("v{version}.json"));
        let generated =
            rendered_schema(version).with_context(|| format!("generate schema v{version}"))?;
        if bless {
            fs::write(&target, &generated)
                .with_context(|| format!("write {}", target.display()))?;
            println!("wrote {} ({} bytes)", target.display(), generated.len());
        } else {
            let on_disk = fs::read_to_string(&target).with_context(|| {
                format!("read {} (run with --bless to create)", target.display())
            })?;
            if on_disk != generated {
                bail!(
                    "schema/v{version}.json drifts from the current types\n\
                     run `cargo xtask gen-schema --bless` to refresh; \
                     reviewers must verify the diff",
                );
            }
            println!("verified {}", target.display());
        }
    }
    Ok(())
}

fn workspace_schema_dir() -> PathBuf {
    workspace_root().join("crates/backhopper-cli/schema")
}

fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("xtask is two levels deep from workspace root")
        .to_path_buf()
}
