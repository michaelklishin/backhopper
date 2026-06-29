// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Formatter dispatch for the CLI: a single envelope shape for JSON,
//! a closure for text. The JSON body always carries `schema_version`,
//! `command`, `data`, and `exit_code` so clients can parse without
//! conditionals.

use std::fmt::Display;
use std::io::{self, Write};

use backhopper_core::schema::CURRENT_SCHEMA_VERSION;
use serde::Serialize;
use serde_json::json;

use crate::cli::Formatter;
use crate::errors::{CliError, CliResult};

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
}

pub fn render<T, FT>(out: &OutputContext, payload: &T, text_render: FT) -> CliResult<()>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    render_with_exit(out, payload, 0, text_render).map(|_| ())
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
/// unchanged; the text path writes `value` directly. Returns exit 0.
pub fn render_display<T>(out: &OutputContext, value: &T) -> CliResult<i32>
where
    T: Serialize + Display,
{
    render_with_exit(out, value, 0, |w| {
        write!(w, "{value}")?;
        Ok(())
    })
}

pub fn render_with_exit<T, FT>(
    out: &OutputContext,
    payload: &T,
    exit_code: i32,
    text_render: FT,
) -> CliResult<i32>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    let mut stdout = io::stdout().lock();
    match out.formatter {
        // markdown and summary fall back to text; bespoke summary projection runs before this call
        Formatter::Json => write_json_envelope(&mut stdout, out, payload, exit_code)?,
        Formatter::Text | Formatter::Markdown | Formatter::Summary | Formatter::TextSummary => {
            text_render(&mut stdout)?;
        }
    }
    Ok(exit_code)
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
    let body = json!({
        "schema_version": out.schema_version,
        "command":        out.command,
        "data":           payload,
        "exit_code":      exit_code,
    });
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
    exit_code: i32,
    text_render: FT,
    markdown_render: FM,
) -> CliResult<i32>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
    FM: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    let mut stdout = io::stdout().lock();
    match out.formatter {
        Formatter::Json => write_json_envelope(&mut stdout, out, payload, exit_code)?,
        Formatter::Text | Formatter::Summary | Formatter::TextSummary => {
            text_render(&mut stdout)?;
        }
        Formatter::Markdown => markdown_render(&mut stdout)?,
    }
    Ok(exit_code)
}
