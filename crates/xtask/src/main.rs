// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Workspace helper tasks.
//!
//! `gen-schema` generates the JSON-schema document(s) `backhopper schema
//! show <N>` serves and verifies the on-disk files match the types.
//! `eval-corpus` folds a corpus of paired verdicts and build outcomes
//! into accuracy rates and a harvest worklist.
//! `install` builds the release binary, copies it into place, ad-hoc
//! re-signs it, and runs it once to absorb the macOS first-run scan so
//! later launches are instant.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

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
    /// Build `backhopper` in release, copy it to `--dest`, ad-hoc
    /// re-sign it, and run it once so the macOS first-run scan is paid
    /// here rather than on first use of the installed binary.
    Install {
        /// Destination path for the installed binary. Defaults to
        /// `$HOME/bin/backhopper`.
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Skip the ad-hoc `codesign` step.
        #[arg(long)]
        no_sign: bool,
        /// Skip the warm-up run.
        #[arg(long)]
        no_warmup: bool,
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
        Command::Install {
            dest,
            no_sign,
            no_warmup,
        } => match run_install(dest, no_sign, no_warmup) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask install: {e:?}");
                ExitCode::from(1)
            }
        },
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

fn run_install(dest: Option<PathBuf>, no_sign: bool, no_warmup: bool) -> Result<()> {
    run_command(ProcessCommand::new("cargo").args(["build", "--release", "-p", "backhopper-cli"]))?;

    let built = workspace_root().join("target/release/backhopper");
    let dest = match dest {
        Some(path) => path,
        None => default_install_dest()?,
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::copy(&built, &dest)
        .with_context(|| format!("copy {} to {}", built.display(), dest.display()))?;
    println!("installed {}", dest.display());

    // `codesign` is macOS-only; the guard keeps signing a no-op elsewhere.
    if !no_sign && cfg!(target_os = "macos") {
        run_command(
            ProcessCommand::new("codesign")
                .args(["--force", "--sign", "-"])
                .arg(&dest),
        )?;
        println!("re-signed {}", dest.display());
    }

    // A new binary's first run triggers a synchronous macOS malware scan
    // that can stall for tens of seconds. We use `codesign` proactively
    // because a Developer Tools exemption is not a given.
    if !no_warmup {
        println!(
            "warming up {} (absorbing the macOS first-run scan)",
            dest.display()
        );
        run_command(ProcessCommand::new(&dest).arg("--version"))?;
        println!("warmed up {}", dest.display());
    }
    Ok(())
}

fn default_install_dest() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join("bin/backhopper"))
}

fn run_command(command: &mut ProcessCommand) -> Result<()> {
    let program = command.get_program().to_owned();
    let status = command
        .status()
        .with_context(|| format!("spawn {}", program.to_string_lossy()))?;
    if !status.success() {
        bail!("{} exited with {status}", program.to_string_lossy());
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
