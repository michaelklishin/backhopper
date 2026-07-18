// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use crate::outcome::CommandOutcome;
use std::path::PathBuf;

use serde::Serialize;

use crate::cli::{ConfigCmd, GlobalArgs};
use crate::commands::context::{load_config, resolve_config_path};
use crate::errors::CliResult;
use crate::output::{OutputContext, render};

#[derive(Debug, Serialize)]
struct PathPayload {
    config_path: PathBuf,
}

pub fn handle(args: &GlobalArgs, cmd: ConfigCmd) -> CliResult<CommandOutcome> {
    match cmd {
        ConfigCmd::Path => {
            let p = resolve_config_path(args)?;
            let payload = PathPayload {
                config_path: p.clone(),
            };
            let ctx = OutputContext::new(args.formatter, "config path");
            render(&ctx, &payload, |w| {
                writeln!(w, "{}", p.display())?;
                Ok(())
            })?;
            Ok(CommandOutcome::Success)
        }
        ConfigCmd::Show => {
            let cfg = load_config(args)?;
            let ctx = OutputContext::new(args.formatter, "config show");
            render(&ctx, &cfg, |w| {
                writeln!(w, "config_path     {}", cfg.config_path.display())?;
                writeln!(w, "snapshot_dir    {}", cfg.snapshot_dir().display())?;
                writeln!(w, "fallback_branch {}", cfg.defaults.fallback_branch)?;
                writeln!(w, "scan_paths      {:?}", cfg.defaults.scan_paths)?;
                for p in &cfg.projects {
                    let url = p
                        .git_url
                        .as_ref()
                        .map(|g| g.display().to_string())
                        .unwrap_or_else(|| format!("self: {}", p.kind.as_str()));
                    writeln!(w, "project   {} ({})", p.name, url)?;
                }
                for s in &cfg.series {
                    writeln!(w, "series    {} ({} pins)", s.name, s.pins.len())?;
                }
                Ok(())
            })?;
            Ok(CommandOutcome::Success)
        }
        ConfigCmd::Validate => {
            load_config(args)?;
            let ctx = OutputContext::new(args.formatter, "config validate");
            render(&ctx, &serde_json::json!({"ok": true}), |w| {
                writeln!(w, "ok")?;
                Ok(())
            })?;
            Ok(CommandOutcome::Success)
        }
    }
}
