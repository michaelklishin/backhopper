// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Projection from evaluation results to `SummaryRow`s, plus emission
//! of the `--formatter summary` (JSONL) and `--formatter text-summary`
//! (tab-separated text) outputs.

use std::collections::BTreeSet;
use std::io::{self, BufWriter, Write};
use std::num::NonZeroU32;

use backhopper_core::config::Config;
use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName};
use backhopper_core::model::summary::{SummaryRow, VerdictKind};
use backhopper_core::model::verdict::{
    SeriesEvaluation, SeriesVerdict, TouchedKinds, Verdict, non_self_tracked,
};

use crate::cli::Formatter;
use crate::errors::{CliError, CliResult};

/// The projects whose pins do not count as tracked-dep evidence: a
/// self pin's scope spans the whole self project, so summing it in
/// would make every self-heavy commit read as harness-verified.
#[must_use]
pub fn self_project_names(cfg: &Config) -> BTreeSet<ProjectName> {
    cfg.projects
        .iter()
        .filter(|p| p.is_self())
        .map(|p| p.name.clone())
        .collect()
}

/// Roll a `SeriesEvaluation` up into one summary row. `tracked` is
/// the saturating sum of non-self pins' `tracked_refs`.
#[must_use]
pub fn to_summary_row(
    eval: &SeriesEvaluation,
    self_projects: &BTreeSet<ProjectName>,
    sha: CommitSha,
    subject: String,
    series: Option<SeriesName>,
    parent_count: Option<NonZeroU32>,
) -> SummaryRow {
    summary_row(
        &eval.verdict,
        self_projects,
        sha,
        subject,
        series,
        parent_count,
    )
}

/// Roll one batch row up into one summary row. Batch rows carry the
/// commit, series, and parent count already.
#[must_use]
pub fn batch_result_to_summary_row(
    result: &BatchResult,
    self_projects: &BTreeSet<ProjectName>,
    subject: String,
) -> SummaryRow {
    summary_row(
        &result.verdict,
        self_projects,
        result.commit.clone(),
        subject,
        Some(result.series.clone()),
        result.parent_count,
    )
}

fn summary_row(
    verdict: &SeriesVerdict,
    self_projects: &BTreeSet<ProjectName>,
    sha: CommitSha,
    subject: String,
    series: Option<SeriesName>,
    parent_count: Option<NonZeroU32>,
) -> SummaryRow {
    let tracked = non_self_tracked(&verdict.results, self_projects);
    SummaryRow {
        sha,
        verdict: rollup_verdict_kind(verdict),
        touched: rollup_touched(verdict),
        tracked,
        subject,
        series,
        parent_count,
    }
}

/// Reduce the per-pin verdicts to a single `VerdictKind` for the row.
///
/// Priority: any `Incompatible` → `Incompatible`; else any
/// `RequiresAdaptation` → `RequiresAdaptation`; else any `Compatible`
/// → `Compatible`; else `Inapplicable`.
fn rollup_verdict_kind(verdict: &SeriesVerdict) -> VerdictKind {
    let mut any_compat = false;
    let mut any_requires = false;
    for pin in &verdict.results {
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

fn rollup_touched(verdict: &SeriesVerdict) -> TouchedKinds {
    verdict
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
        let series = row.series.as_ref().map_or("-", |s| s.as_str());
        writeln!(
            w,
            "{}\t{}\t{}\t{}\t{}\t{}",
            row.sha.abbreviated(),
            row.verdict.as_str(),
            touched,
            row.tracked,
            series,
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
