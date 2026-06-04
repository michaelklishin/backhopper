// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Projection from a `SeriesEvaluation` to one `SummaryRow`, plus
//! emission of the `--formatter summary` (JSONL) and
//! `--formatter text-summary` (tab-separated text) outputs.

use std::io::{self, BufWriter, Write};

use backhopper_core::model::names::CommitSha;
use backhopper_core::model::summary::{SummaryRow, VerdictKind};
use backhopper_core::model::verdict::{SeriesEvaluation, TouchedKinds, Verdict};

use crate::cli::Formatter;
use crate::errors::{CliError, CliResult};

/// Roll a `SeriesEvaluation` up into one summary row. `tracked` is
/// the saturating sum of per-pin `tracked_refs`.
#[must_use]
pub fn to_summary_row(eval: &SeriesEvaluation, sha: CommitSha, subject: String) -> SummaryRow {
    let verdict = rollup_verdict_kind(eval);
    let touched = rollup_touched(eval);
    let tracked: u32 = eval.verdict.results.iter().fold(0u32, |acc, pin| {
        let v = u32::try_from(pin.tracked_refs).unwrap_or(u32::MAX);
        acc.saturating_add(v)
    });
    SummaryRow {
        sha,
        verdict,
        touched,
        tracked,
        subject,
    }
}

/// Reduce the per-pin verdicts to a single `VerdictKind` for the row.
///
/// Priority: any `Incompatible` → `Incompatible`; else any
/// `RequiresAdaptation` → `RequiresAdaptation`; else any `Compatible`
/// → `Compatible`; else `Inapplicable`.
fn rollup_verdict_kind(eval: &SeriesEvaluation) -> VerdictKind {
    let mut any_compat = false;
    let mut any_requires = false;
    for pin in &eval.verdict.results {
        match pin.verdict {
            Verdict::Incompatible { .. } => return VerdictKind::Incompatible,
            Verdict::RequiresAdaptation { .. } => any_requires = true,
            Verdict::Compatible => any_compat = true,
            Verdict::Inapplicable { .. } => {}
        }
    }
    if any_requires {
        VerdictKind::RequiresAdaptation
    } else if any_compat {
        VerdictKind::Compatible
    } else {
        VerdictKind::Inapplicable
    }
}

fn rollup_touched(eval: &SeriesEvaluation) -> TouchedKinds {
    eval.verdict
        .results
        .iter()
        .map(|p| p.touched)
        .find(|t| !t.is_empty())
        .unwrap_or_default()
}

/// Which summary projection to emit. Built only from a `Formatter`
/// that already names a summary mode, so `emit_rows` cannot be called
/// with a non-summary formatter at compile time.
#[derive(Debug, Clone, Copy)]
pub enum SummaryFormatter {
    Jsonl,
    Text,
}

impl SummaryFormatter {
    #[must_use]
    pub fn from_cli(f: Formatter) -> Option<Self> {
        match f {
            Formatter::Summary => Some(Self::Jsonl),
            Formatter::TextSummary => Some(Self::Text),
            Formatter::Json | Formatter::Text | Formatter::Markdown => None,
        }
    }
}

/// Write a slice of summary rows in either JSONL (one object per
/// line, trailing newline, no array wrap) or tab-separated text.
pub fn emit_rows(formatter: SummaryFormatter, rows: &[SummaryRow]) -> CliResult<()> {
    let stdout = io::stdout().lock();
    let mut buf = BufWriter::new(stdout);
    let result = match formatter {
        SummaryFormatter::Jsonl => emit_jsonl(&mut buf, rows),
        SummaryFormatter::Text => emit_text(&mut buf, rows),
    };
    buf.flush()?;
    result
}

fn emit_jsonl(w: &mut dyn Write, rows: &[SummaryRow]) -> CliResult<()> {
    for row in rows {
        let bytes = serde_json::to_vec(row).map_err(|e| CliError::OutputError(e.to_string()))?;
        w.write_all(&bytes)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

fn emit_text(w: &mut dyn Write, rows: &[SummaryRow]) -> CliResult<()> {
    for row in rows {
        let touched = render_touched_short(&row.touched);
        writeln!(
            w,
            "{}\t{}\t{}\t{}\t{}",
            row.sha.abbreviated(),
            row.verdict.as_str(),
            touched,
            row.tracked,
            row.subject.replace('\t', " "),
        )?;
    }
    Ok(())
}

fn render_touched_short(t: &TouchedKinds) -> String {
    let kinds: [(&str, u32); 11] = [
        ("erl", t.erl),
        ("hrl", t.hrl),
        ("tests", t.tests),
        ("schema", t.schema),
        ("docs", t.docs),
        ("makefile", t.makefile),
        ("mix_exs", t.mix_exs),
        ("ci_workflow", t.ci_workflow),
        ("app_src", t.app_src),
        ("rebar_config", t.rebar_config),
        ("other", t.other),
    ];
    let parts: Vec<String> = kinds
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(name, n)| format!("{name}={n}"))
        .collect();
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(",")
    }
}
