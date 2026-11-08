// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::io::Write;
use std::ops::Range;

use bel7_cli::TableStyle;

use backhopper_core::jigsaw::findings::WithheldReason;
use backhopper_core::jigsaw::references::NotChecked;
use backhopper_core::jigsaw::report::JigsawCell;
use backhopper_core::jigsaw::{
    JigsawCheckPayload, JigsawPlanPayload, JigsawRangePayload, MissingSnapshotPayload,
    PlanReleasePayload, TagSourceKind,
};

use crate::errors::CliResult;
use crate::output::plural;
use crate::tables::styled_grid;

fn run_label(first: &str, last: &str) -> String {
    if first == last {
        first.to_owned()
    } else {
        format!("{first} .. {last}")
    }
}

fn span_label<T: AsRef<str>>(items: &[T]) -> Option<String> {
    Some(run_label(items.first()?.as_ref(), items.last()?.as_ref()))
}

fn ranges_label(ranges: &[JigsawRangePayload]) -> String {
    ranges
        .iter()
        .map(|range| run_label(&range.first, &range.last))
        .collect::<Vec<_>>()
        .join(", ")
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn markdown_table(rows: &[Vec<String>]) -> Vec<String> {
    let Some((header, body)) = rows.split_first() else {
        return Vec::new();
    };
    let line = |cells: &[String]| {
        let escaped: Vec<String> = cells.iter().map(|cell| escape_cell(cell)).collect();
        format!("| {} |", escaped.join(" | "))
    };
    let mut lines = vec![line(header), format!("|{}", " --- |".repeat(header.len()))];
    lines.extend(body.iter().map(|row| line(row)));
    lines
}

fn not_checked_phrase(reason: NotChecked, build_release: &str) -> String {
    match reason {
        NotChecked::Internal => "call sites resolved inside the plugin set".to_owned(),
        NotChecked::Dynamic => "dynamic calls".to_owned(),
        NotChecked::UnresolvedAtBuild => format!("not resolved at {build_release}"),
        NotChecked::Elixir => "calls into Elixir modules".to_owned(),
        NotChecked::Untracked => "references into untracked dependencies".to_owned(),
        NotChecked::UnresolvedInclude => {
            "files with an OTP header the reader didn't see".to_owned()
        }
        NotChecked::ConditionalSite => {
            "references used only under a version conditional".to_owned()
        }
    }
}

fn withheld_phrase(reason: WithheldReason) -> &'static str {
    match reason {
        WithheldReason::ConditionalMacro => "conditional macros",
        WithheldReason::UnknownSpec => "unknown specs",
        WithheldReason::UnreadableManifest => "unreadable manifests",
        WithheldReason::UntrackedApp => "apps of untracked dependencies",
        WithheldReason::ExcludedOtpApp => "OTP apps the snapshot excludes",
    }
}

fn tag_source_note(source: TagSourceKind) -> Option<&'static str> {
    match source {
        TagSourceKind::Repository => None,
        TagSourceKind::Store => Some(
            "host releases listed from the snapshot store; a release with no snapshot yet is not included",
        ),
    }
}

struct MatrixColumn {
    label: String,
    cells: Vec<Option<JigsawCell>>,
}

/// Columns are runs of host releases with the same state for every app. A
/// run splits by OTP tags only when the state differs between OTP tags.
fn matrix_columns(payload: &JigsawCheckPayload) -> Vec<MatrixColumn> {
    let hosts = payload.host_releases.len();
    let otp = payload.otp_tags.len();
    let state = |host: usize, tag: usize| -> Vec<Option<JigsawCell>> {
        payload
            .apps
            .iter()
            .map(|row| row.cells.get(host * otp + tag).copied())
            .collect()
    };
    let starts_group =
        |tag: usize| tag == 0 || (0..hosts).any(|host| state(host, tag) != state(host, tag - 1));
    let tag_groups = runs((0..otp).filter(|&tag| starts_group(tag)).collect(), otp);
    let starts_run = |host: usize| {
        host == 0
            || tag_groups
                .iter()
                .any(|group| state(host, group.start) != state(host - 1, group.start))
    };
    let host_runs = runs((0..hosts).filter(|&host| starts_run(host)).collect(), hosts);
    host_runs
        .iter()
        .flat_map(|host_run| {
            tag_groups.iter().map(|tag_group| {
                let host_label = run_label(
                    &payload.host_releases[host_run.start],
                    &payload.host_releases[host_run.end - 1],
                );
                let label = if tag_groups.len() == 1 {
                    host_label
                } else {
                    let tags = run_label(
                        &payload.otp_tags[tag_group.start],
                        &payload.otp_tags[tag_group.end - 1],
                    );
                    format!("{host_label}, {tags}")
                };
                MatrixColumn {
                    label,
                    cells: state(host_run.start, tag_group.start),
                }
            })
        })
        .collect()
}

fn runs(starts: Vec<usize>, end: usize) -> Vec<Range<usize>> {
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| start..starts.get(i + 1).copied().unwrap_or(end))
        .collect()
}

struct EntryView {
    kind: String,
    symbol: String,
    detail: Option<String>,
    scope: String,
    sites: Vec<String>,
}

struct UncheckedGroup {
    heading: String,
    lines: Vec<String>,
}

struct CheckView {
    title: String,
    targets: String,
    matrix: Vec<Vec<String>>,
    findings: Vec<EntryView>,
    warnings: Vec<EntryView>,
    checked: String,
    not_checked: Vec<String>,
    withheld: Vec<String>,
    unchecked: Vec<UncheckedGroup>,
    withheld_entries: Vec<EntryView>,
    host_provided: Option<String>,
    limits: String,
}

fn scope_label(host: &[JigsawRangePayload], otp: &[JigsawRangePayload]) -> String {
    if !otp.is_empty() {
        format!("OTP tags: {}  (all host releases)", ranges_label(otp))
    } else if !host.is_empty() {
        format!("host releases: {}  (all OTP tags)", ranges_label(host))
    } else {
        "every target".to_owned()
    }
}

fn tally_lines<K: Copy + Ord>(
    all: &[K],
    counts: &BTreeMap<K, u64>,
    phrase: impl Fn(K) -> String,
) -> Vec<String> {
    all.iter()
        .map(|&reason| {
            let n = counts.get(&reason).copied().unwrap_or(0);
            format!("{}: {}", phrase(reason), thousands(n))
        })
        .collect()
}

fn unchecked_groups(payload: &JigsawCheckPayload) -> Vec<UncheckedGroup> {
    NotChecked::ALL
        .iter()
        .filter_map(|&reason| {
            let lines: Vec<String> = payload
                .unchecked
                .iter()
                .filter(|site| site.reason == reason)
                .map(|site| format!("{}/{}:{}  {}", site.app, site.path, site.line, site.symbol))
                .collect();
            (!lines.is_empty()).then(|| UncheckedGroup {
                heading: not_checked_phrase(reason, &payload.build_release),
                lines,
            })
        })
        .collect()
}

impl CheckView {
    fn of(payload: &JigsawCheckPayload) -> Self {
        let columns = matrix_columns(payload);
        let mut header = vec!["app".to_owned()];
        header.extend(columns.iter().map(|column| column.label.clone()));
        let mut matrix = vec![header];
        for (i, row) in payload.apps.iter().enumerate() {
            let mut line = vec![row.app.to_string()];
            line.extend(columns.iter().map(|column| {
                column
                    .cells
                    .get(i)
                    .copied()
                    .flatten()
                    .map_or_else(|| "unknown".to_owned(), |cell| cell.to_string())
            }));
            matrix.push(line);
        }
        let targets = format!(
            "targets: {} host release{} ({}) x {} OTP tag{} ({}){}",
            payload.host_releases.len(),
            plural(payload.host_releases.len()),
            span_label(&payload.host_releases).unwrap_or_default(),
            payload.otp_tags.len(),
            plural(payload.otp_tags.len()),
            span_label(&payload.otp_tags).unwrap_or_default(),
            tag_source_note(payload.tag_source)
                .map(|note| format!(" ({note})"))
                .unwrap_or_default(),
        );
        Self {
            title: format!(
                "jigsaw check: {} {}, built against {}",
                payload.host, payload.series, payload.build_release
            ),
            targets,
            matrix,
            findings: payload
                .findings
                .iter()
                .map(|finding| EntryView {
                    kind: finding.kind.to_string(),
                    symbol: finding.symbol.clone(),
                    detail: finding.detail.clone(),
                    scope: scope_label(&finding.host_release_ranges, &finding.otp_tag_ranges),
                    sites: finding
                        .sites
                        .iter()
                        .map(|site| format!("{}  {}:{}", site.app, site.path, site.line))
                        .collect(),
                })
                .collect(),
            warnings: payload
                .warnings
                .iter()
                .map(|warning| EntryView {
                    kind: warning.kind.to_string(),
                    symbol: warning.symbol.clone(),
                    detail: warning.detail.clone(),
                    scope: scope_label(&warning.host_release_ranges, &[]),
                    sites: Vec::new(),
                })
                .collect(),
            checked: format!(
                "{} distinct references to the host and OTP, checked at every target",
                thousands(payload.tallies.checked)
            ),
            not_checked: tally_lines(NotChecked::ALL, &payload.tallies.not_checked, |reason| {
                not_checked_phrase(reason, &payload.build_release)
            }),
            withheld: tally_lines(WithheldReason::ALL, &payload.tallies.withheld, |reason| {
                withheld_phrase(reason).to_owned()
            }),
            unchecked: unchecked_groups(payload),
            withheld_entries: payload
                .withheld
                .iter()
                .map(|withheld| EntryView {
                    kind: withheld.reason.to_string(),
                    symbol: withheld.symbol.clone(),
                    detail: None,
                    scope: scope_label(&withheld.host_release_ranges, &withheld.otp_tag_ranges),
                    sites: withheld
                        .sites
                        .iter()
                        .map(|site| format!("{}  {}:{}", site.app, site.path, site.line))
                        .collect(),
                })
                .collect(),
            host_provided: (!payload.host_provided.is_empty()).then(|| {
                format!(
                    "host apps found in plugin deps/ and left out: {}",
                    payload.host_provided.join(", ")
                )
            }),
            limits: format!("limits: {}", payload.limits.join("; ")),
        }
    }
}

fn count_heading(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", plural(n))
}

fn write_entry_text(w: &mut dyn Write, entry: &EntryView) -> CliResult<()> {
    match &entry.detail {
        Some(detail) => writeln!(w, "{}  {}  ({detail})", entry.kind, entry.symbol)?,
        None => writeln!(w, "{}  {}", entry.kind, entry.symbol)?,
    }
    writeln!(w, "  {}", entry.scope)?;
    for site in &entry.sites {
        writeln!(w, "  {site}")?;
    }
    Ok(())
}

pub fn write_check_text(
    w: &mut dyn Write,
    payload: &JigsawCheckPayload,
    style: TableStyle,
) -> CliResult<()> {
    let view = CheckView::of(payload);
    writeln!(w, "{}", view.title)?;
    writeln!(w, "{}", view.targets)?;
    writeln!(w)?;
    writeln!(w, "{}", styled_grid(view.matrix, style))?;
    writeln!(w)?;
    writeln!(w, "{}", count_heading(view.findings.len(), "finding"))?;
    for entry in &view.findings {
        writeln!(w)?;
        write_entry_text(w, entry)?;
    }
    if !view.warnings.is_empty() {
        writeln!(w)?;
        writeln!(w, "{}", count_heading(view.warnings.len(), "warning"))?;
        for entry in &view.warnings {
            writeln!(w)?;
            write_entry_text(w, entry)?;
        }
    }
    writeln!(w)?;
    writeln!(w, "{}", view.checked)?;
    writeln!(w, "not checked:")?;
    for line in &view.not_checked {
        writeln!(w, "  {line}")?;
    }
    writeln!(w, "withheld:")?;
    for line in &view.withheld {
        writeln!(w, "  {line}")?;
    }
    for group in &view.unchecked {
        writeln!(w)?;
        writeln!(w, "{}:", group.heading)?;
        for line in &group.lines {
            writeln!(w, "  {line}")?;
        }
    }
    if !view.unchecked.is_empty() {
        writeln!(w)?;
    }
    if !view.withheld_entries.is_empty() {
        writeln!(w, "comparisons withheld:")?;
        for entry in &view.withheld_entries {
            writeln!(w)?;
            write_entry_text(w, entry)?;
        }
        writeln!(w)?;
    }
    if let Some(line) = &view.host_provided {
        writeln!(w, "{line}")?;
    }
    writeln!(w)?;
    writeln!(w, "{}", view.limits)?;
    Ok(())
}

fn write_entry_markdown(w: &mut dyn Write, entry: &EntryView) -> CliResult<()> {
    writeln!(w, "#### {} `{}`", entry.kind, entry.symbol)?;
    writeln!(w)?;
    if let Some(detail) = &entry.detail {
        writeln!(w, "* {detail}")?;
    }
    writeln!(w, "* {}", entry.scope)?;
    for site in &entry.sites {
        writeln!(w, "* `{site}`")?;
    }
    writeln!(w)?;
    Ok(())
}

fn write_bullets(w: &mut dyn Write, heading: &str, lines: &[String]) -> CliResult<()> {
    writeln!(w, "{heading}")?;
    writeln!(w)?;
    for line in lines {
        writeln!(w, "* {line}")?;
    }
    writeln!(w)?;
    Ok(())
}

pub fn write_check_markdown(w: &mut dyn Write, payload: &JigsawCheckPayload) -> CliResult<()> {
    let view = CheckView::of(payload);
    writeln!(w, "## {}", view.title)?;
    writeln!(w)?;
    writeln!(w, "{}", view.targets)?;
    writeln!(w)?;
    for line in markdown_table(&view.matrix) {
        writeln!(w, "{line}")?;
    }
    writeln!(w)?;
    writeln!(w, "### {}", count_heading(view.findings.len(), "finding"))?;
    writeln!(w)?;
    for entry in &view.findings {
        write_entry_markdown(w, entry)?;
    }
    if !view.warnings.is_empty() {
        writeln!(w, "### {}", count_heading(view.warnings.len(), "warning"))?;
        writeln!(w)?;
        for entry in &view.warnings {
            write_entry_markdown(w, entry)?;
        }
    }
    writeln!(w, "{}", view.checked)?;
    writeln!(w)?;
    write_bullets(w, "Not checked:", &view.not_checked)?;
    write_bullets(w, "Withheld:", &view.withheld)?;
    for group in &view.unchecked {
        writeln!(w, "{}:", group.heading)?;
        writeln!(w)?;
        for line in &group.lines {
            writeln!(w, "* `{line}`")?;
        }
        writeln!(w)?;
    }
    if !view.withheld_entries.is_empty() {
        writeln!(w, "Comparisons withheld:")?;
        writeln!(w)?;
        for entry in &view.withheld_entries {
            write_entry_markdown(w, entry)?;
        }
    }
    if let Some(line) = &view.host_provided {
        writeln!(w, "{line}")?;
        writeln!(w)?;
    }
    writeln!(w, "{}", view.limits)?;
    Ok(())
}

fn pin_changes(previous: Option<&PlanReleasePayload>, release: &PlanReleasePayload) -> String {
    let Some(previous) = previous else {
        return "-".to_owned();
    };
    let mut changes: Vec<String> = Vec::new();
    for (project, tag) in &release.pins {
        match previous.pins.get(project) {
            Some(before) if before == tag => {}
            Some(before) => changes.push(format!("{project} {before} -> {tag}")),
            None => changes.push(format!("{project} {tag} (new)")),
        }
    }
    for project in previous.pins.keys() {
        if !release.pins.contains_key(project) {
            changes.push(format!("{project} (dropped)"));
        }
    }
    if changes.is_empty() {
        "-".to_owned()
    } else {
        changes.join(", ")
    }
}

struct PlanView {
    title: String,
    releases_line: String,
    releases: Vec<Vec<String>>,
    otp_lines: Vec<String>,
    required: String,
    missing: Vec<String>,
    untracked: Option<String>,
    cache_key: String,
    restore_key: String,
}

impl PlanView {
    fn of(payload: &JigsawPlanPayload) -> Self {
        let tags: Vec<&str> = payload.releases.iter().map(|r| r.tag.as_str()).collect();
        let mut releases = vec![vec![
            "release".to_owned(),
            "erlang".to_owned(),
            "pins_changed".to_owned(),
        ]];
        let mut previous = None;
        for release in &payload.releases {
            releases.push(vec![
                release.tag.clone(),
                release.erlang_support.clone(),
                pin_changes(previous, release),
            ]);
            previous = Some(release);
        }
        Self {
            title: format!(
                "jigsaw plan: {} {}, built against {}",
                payload.host,
                payload.series,
                tags.last().copied().unwrap_or("-")
            ),
            releases_line: format!(
                "{} host release{} ({}), tags listed from the {}",
                tags.len(),
                plural(tags.len()),
                span_label(&tags).unwrap_or_default(),
                payload.tag_source
            ),
            releases,
            otp_lines: payload
                .otp_lines
                .iter()
                .map(|line| {
                    format!(
                        "{}: {} ({})",
                        line.line,
                        span_label(&line.tags).unwrap_or_default(),
                        count_heading(line.tags.len(), "tag")
                    )
                })
                .collect(),
            required: format!(
                "{} required, {} to generate",
                count_heading(payload.required_snapshots.len(), "snapshot"),
                payload.missing_snapshots.len()
            ),
            missing: payload
                .missing_snapshots
                .iter()
                .map(
                    |MissingSnapshotPayload {
                         project,
                         tag,
                         reason,
                         detail,
                     }| match detail {
                        Some(detail) => format!("{reason} {project} {tag}: {detail}"),
                        None => format!("{reason} {project} {tag}"),
                    },
                )
                .collect(),
            untracked: (!payload.untracked_dependencies.is_empty()).then(|| {
                format!(
                    "untracked dependencies: {}",
                    payload.untracked_dependencies.join(", ")
                )
            }),
            cache_key: format!("cache key: {}", payload.cache_key),
            restore_key: format!("restore key: {}", payload.restore_key),
        }
    }
}

pub fn write_plan_text(
    w: &mut dyn Write,
    payload: &JigsawPlanPayload,
    style: TableStyle,
) -> CliResult<()> {
    let view = PlanView::of(payload);
    writeln!(w, "{}", view.title)?;
    writeln!(w, "{}", view.releases_line)?;
    writeln!(w)?;
    writeln!(w, "{}", styled_grid(view.releases, style))?;
    writeln!(w)?;
    writeln!(w, "OTP lines:")?;
    for line in &view.otp_lines {
        writeln!(w, "  {line}")?;
    }
    writeln!(w)?;
    writeln!(w, "{}", view.required)?;
    for line in &view.missing {
        writeln!(w, "  {line}")?;
    }
    if let Some(line) = &view.untracked {
        writeln!(w, "{line}")?;
    }
    writeln!(w, "{}", view.cache_key)?;
    writeln!(w, "{}", view.restore_key)?;
    Ok(())
}

pub fn write_plan_markdown(w: &mut dyn Write, payload: &JigsawPlanPayload) -> CliResult<()> {
    let view = PlanView::of(payload);
    writeln!(w, "## {}", view.title)?;
    writeln!(w)?;
    writeln!(w, "{}", view.releases_line)?;
    writeln!(w)?;
    for line in markdown_table(&view.releases) {
        writeln!(w, "{line}")?;
    }
    writeln!(w)?;
    write_bullets(w, "OTP lines:", &view.otp_lines)?;
    writeln!(w, "{}", view.required)?;
    writeln!(w)?;
    if !view.missing.is_empty() {
        for line in &view.missing {
            writeln!(w, "* {line}")?;
        }
        writeln!(w)?;
    }
    if let Some(line) = &view.untracked {
        writeln!(w, "{line}")?;
        writeln!(w)?;
    }
    writeln!(w, "* {}", view.cache_key)?;
    writeln!(w, "* {}", view.restore_key)?;
    Ok(())
}
