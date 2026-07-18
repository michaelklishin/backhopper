// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Formatter dispatch for the CLI: a single envelope shape for JSON,
//! a closure for text. The JSON body always carries `schema_version`,
//! `command`, `data`, and `exit_code` so clients can parse without
//! conditionals.

use std::fmt::Display;
use std::io::{self, Write};

use backhopper_core::model::wire_envelope::WireEnvelope;
use backhopper_core::schema::CURRENT_SCHEMA_VERSION;
use serde::Serialize;

use crate::cli::{Formatter, GlobalArgs};
use crate::errors::{CliError, CliResult};
use crate::outcome::CommandOutcome;

pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[derive(Debug)]
pub struct OutputContext {
    pub formatter: Formatter,
    pub command: &'static str,
    pub schema_version: u32,
}

impl OutputContext {
    pub fn new(formatter: Formatter, command: &'static str) -> Self {
        Self {
            formatter,
            command,
            schema_version: CURRENT_SCHEMA_VERSION,
        }
    }

    pub fn from_args(global: &GlobalArgs, command: &'static str) -> Self {
        Self::new(global.formatter, command)
    }
}

pub fn render<T, FT>(out: &OutputContext, payload: &T, text_render: FT) -> CliResult<()>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    render_with_exit(out, payload, CommandOutcome::Success, text_render).map(|_| ())
}

/// Write one JSON-serialized row per line (JSONL): no array wrap, a
/// trailing newline after each row.
pub fn emit_jsonl<T>(w: &mut dyn Write, rows: &[T]) -> CliResult<()>
where
    T: Serialize,
{
    for row in rows {
        let bytes = serde_json::to_vec(row).map_err(|e| CliError::OutputError(e.to_string()))?;
        w.write_all(&bytes)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

/// Render a value whose text form is its `Display` impl. JSON path is
/// unchanged; the text path writes `value` directly. Always succeeds.
pub fn render_display<T>(out: &OutputContext, value: &T) -> CliResult<CommandOutcome>
where
    T: Serialize + Display,
{
    render_with_exit(out, value, CommandOutcome::Success, |w| {
        write!(w, "{value}")?;
        Ok(())
    })
}

pub fn render_with_exit<T, FT>(
    out: &OutputContext,
    payload: &T,
    outcome: CommandOutcome,
    text_render: FT,
) -> CliResult<CommandOutcome>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    let mut stdout = io::stdout().lock();
    match out.formatter {
        // markdown and summary fall back to text; bespoke summary projection runs before this call
        Formatter::Json => write_json_envelope(&mut stdout, out, payload, outcome.exit_code())?,
        Formatter::Text | Formatter::Markdown | Formatter::Summary | Formatter::TextSummary => {
            text_render(&mut stdout)?;
        }
    }
    Ok(outcome)
}

fn write_json_envelope<T, W>(
    stdout: &mut W,
    out: &OutputContext,
    payload: &T,
    exit_code: i32,
) -> CliResult<()>
where
    T: Serialize,
    W: Write,
{
    let envelope = WireEnvelope {
        schema_version: out.schema_version,
        command: Some(out.command.to_owned()),
        data: payload,
        exit_code,
        warnings: Vec::new(),
    };
    // through a Value so the keys serialize sorted, matching the shape
    // the driver parses and `schema show` documents
    let body = serde_json::to_value(&envelope).map_err(|e| CliError::OutputError(e.to_string()))?;
    serde_json::to_writer_pretty(&mut *stdout, &body)
        .map_err(|e| CliError::OutputError(e.to_string()))?;
    writeln!(stdout)?;
    Ok(())
}

/// Variant that lets a command supply a markdown-specific closure. The
/// JSON path is unchanged; `Text` uses `text_render`; `Markdown` uses
/// `markdown_render`.
pub fn render_with_alts<T, FT, FM>(
    out: &OutputContext,
    payload: &T,
    outcome: CommandOutcome,
    text_render: FT,
    markdown_render: FM,
) -> CliResult<CommandOutcome>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
    FM: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    let mut stdout = io::stdout().lock();
    match out.formatter {
        Formatter::Json => write_json_envelope(&mut stdout, out, payload, outcome.exit_code())?,
        Formatter::Text | Formatter::Summary | Formatter::TextSummary => {
            text_render(&mut stdout)?;
        }
        Formatter::Markdown => markdown_render(&mut stdout)?,
    }
    Ok(outcome)
}
