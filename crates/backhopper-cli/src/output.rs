//! Output formatter dispatch: JSON vs text.

use std::io::Write;

use serde::Serialize;
use serde_json::json;

use crate::cli::Formatter;
use crate::errors::{CliError, CliResult};

#[derive(Debug)]
pub struct OutputContext {
    pub formatter: Formatter,
    pub command: &'static str,
    pub schema_version: u32,
    pub table_style: String,
}

impl OutputContext {
    pub fn new(formatter: Formatter, command: &'static str) -> Self {
        Self {
            formatter,
            command,
            schema_version: 1,
            table_style: String::from("modern"),
        }
    }
}

pub fn render<T, FT>(out: &OutputContext, payload: &T, text_render: FT) -> CliResult<()>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    let mut stdout = std::io::stdout().lock();
    match out.formatter {
        Formatter::Json => {
            let body = json!({
                "schema_version": out.schema_version,
                "command":        out.command,
                "data":           payload,
            });
            serde_json::to_writer_pretty(&mut stdout, &body)
                .map_err(|e| CliError::OutputError(e.to_string()))?;
            writeln!(stdout)?;
        }
        Formatter::Text => {
            text_render(&mut stdout)?;
        }
    }
    Ok(())
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
    let mut stdout = std::io::stdout().lock();
    match out.formatter {
        Formatter::Json => {
            let body = json!({
                "schema_version": out.schema_version,
                "command":        out.command,
                "data":           payload,
                "exit_code":      exit_code,
            });
            serde_json::to_writer_pretty(&mut stdout, &body)
                .map_err(|e| CliError::OutputError(e.to_string()))?;
            writeln!(stdout)?;
        }
        Formatter::Text => {
            text_render(&mut stdout)?;
        }
    }
    Ok(exit_code)
}
