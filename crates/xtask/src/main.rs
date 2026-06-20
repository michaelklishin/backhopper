// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Workspace helper tasks.
//!
//! `gen-schema` generates the JSON-schema document(s) `backhopper schema
//! show <N>` serves and verifies the on-disk files match the types.
//! `eval-corpus` folds a corpus of paired verdicts and build outcomes
//! into accuracy rates and a harvest worklist.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use backhopper_core::model::eval::{CorpusEntry, evaluate_corpus};
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
    /// Fold a JSON corpus of `(fingerprint, verdict, outcome)` rows into
    /// recall, precision, the vacuous-trust rate, and a list of breaks a
    /// clean verdict missed. Prints the report as JSON.
    EvalCorpus {
        /// Path to the corpus JSON: an array of `CorpusEntry`.
        #[arg(long)]
        corpus: PathBuf,
        /// When set, write the missed-break worklist here as JSON.
        #[arg(long)]
        harvest: Option<PathBuf>,
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
        Command::EvalCorpus { corpus, harvest } => {
            match run_eval_corpus(&corpus, harvest.as_deref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("xtask eval-corpus: {e:?}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn run_eval_corpus(corpus: &Path, harvest: Option<&Path>) -> Result<()> {
    let bytes = fs::read(corpus).with_context(|| format!("read corpus {}", corpus.display()))?;
    let entries: Vec<CorpusEntry> =
        serde_json::from_slice(&bytes).context("parse corpus as a CorpusEntry array")?;
    let report = evaluate_corpus(&entries);
    println!("{}", serde_json::to_string_pretty(&report)?);
    if let Some(path) = harvest {
        let body = serde_json::to_vec_pretty(&report.missed_breaks)?;
        fs::write(path, body).with_context(|| format!("write harvest {}", path.display()))?;
    }
    Ok(())
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
