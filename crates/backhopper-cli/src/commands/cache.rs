// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The `cache` verb group: inspection and maintenance of both
//! workspace caches (the verdict cache and the siblings doctor
//! cache). Deliberately tiny; every operation is scan-and-filter
//! over self-describing entries.

use crate::outcome::CommandOutcome;
use std::io::Write;
use std::time::Duration;

use bel7_cli::TableStyle;
use tabled::Tabled;

use backhopper_cache::{
    KeyLookupError, ScannedEntry, WorkspaceCaches, clear, evict_entry, find_by_key_prefix, prune,
    scan, stats,
};
use backhopper_core::model::cache::{
    CacheListPayload, CacheListRow, CacheMutationPayload, CacheShowPayload, CacheStatsPayload,
};
use backhopper_core::model::names::{CommitShaPrefix, SeriesName};

use crate::cli::{CacheCmd, GlobalArgs};
use crate::commands::context::{load_config, snapshot_dir};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};
use crate::tables::styled_table;

/// The shortest accepted cache-key prefix, matching the commit-SHA
/// prefix affordance.
const MIN_KEY_PREFIX_LEN: usize = 8;

fn commit_prefix_matches(entry: &ScannedEntry, prefix: &CommitShaPrefix) -> bool {
    entry
        .commit()
        .is_some_and(|sha| sha.starts_with(prefix.as_str()))
}

pub fn handle(args: &GlobalArgs, cmd: CacheCmd) -> CliResult<CommandOutcome> {
    let cfg = load_config(args)?;
    let caches = WorkspaceCaches::at(&snapshot_dir(args, &cfg));
    match cmd {
        CacheCmd::Stats => run_stats(args, &caches),
        CacheCmd::List {
            commit,
            series,
            min_age_days,
        } => run_list(
            args,
            &caches,
            commit.as_ref(),
            series.as_ref(),
            min_age_days,
        ),
        CacheCmd::Show { key_prefix, full } => run_show(args, &caches, &key_prefix, full),
        CacheCmd::Evict {
            key_prefixes,
            commit,
        } => run_evict(args, &caches, &key_prefixes, commit.as_ref()),
        CacheCmd::Prune { older_than } => run_prune(args, &caches, older_than),
        CacheCmd::Clear => run_clear(args, &caches),
    }
}

#[derive(Tabled)]
struct StatsRow {
    #[tabled(rename = "LEVEL")]
    level: &'static str,
    #[tabled(rename = "ENTRIES")]
    entries: u64,
    #[tabled(rename = "BYTES")]
    bytes: u64,
}

fn run_stats(args: &GlobalArgs, caches: &WorkspaceCaches) -> CliResult<CommandOutcome> {
    let payload: CacheStatsPayload = stats(caches);
    let style = args.table_style;
    let ctx = OutputContext::new(args.formatter, "cache stats");
    render(&ctx, &payload, |w| {
        let rows = vec![
            StatsRow {
                level: "by-input",
                entries: payload.by_input.entries,
                bytes: payload.by_input.bytes,
            },
            StatsRow {
                level: "by-content",
                entries: payload.by_content.entries,
                bytes: payload.by_content.bytes,
            },
            StatsRow {
                level: "siblings",
                entries: payload.siblings.entries,
                bytes: payload.siblings.bytes,
            },
        ];
        writeln!(w, "{}", styled_table(rows, style))?;
        writeln!(
            w,
            "aliases: {}; total bytes: {}",
            payload.aliases, payload.total_bytes
        )?;
        if let (Some(oldest), Some(newest)) = (&payload.oldest_entry_at, &payload.newest_entry_at) {
            writeln!(w, "oldest entry: {oldest}; newest entry: {newest}")?;
        }
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

#[derive(Tabled)]
struct ListRow {
    #[tabled(rename = "KEY")]
    key: String,
    #[tabled(rename = "LEVEL")]
    level: &'static str,
    #[tabled(rename = "COMMIT")]
    commit: String,
    #[tabled(rename = "SERIES")]
    series: String,
    #[tabled(rename = "AGE")]
    age_days: String,
    #[tabled(rename = "BYTES")]
    bytes: u64,
    #[tabled(rename = "ALIAS")]
    alias: String,
}

fn run_list(
    args: &GlobalArgs,
    caches: &WorkspaceCaches,
    commit: Option<&CommitShaPrefix>,
    series: Option<&SeriesName>,
    min_age_days: Option<u32>,
) -> CliResult<CommandOutcome> {
    let entries = scan(caches);
    let rows: Vec<CacheListRow> = entries
        .iter()
        .filter(|e| commit.is_none_or(|c| commit_prefix_matches(e, c)))
        .filter(|e| series.is_none_or(|s| e.series() == Some(s.as_str())))
        .filter(|e| min_age_days.is_none_or(|d| age_days(e) >= d))
        .map(|e| CacheListRow {
            key: e.key.clone(),
            level: e.level,
            commit: e.commit().map(str::to_owned),
            series: e.series().map(str::to_owned),
            age_days: age_days(e),
            bytes: e.bytes,
            alias: e.alias,
        })
        .collect();
    let payload = CacheListPayload { rows };
    let style = args.table_style;
    let ctx = OutputContext::new(args.formatter, "cache list");
    render(&ctx, &payload, |w| {
        if payload.rows.is_empty() {
            writeln!(w, "no cache entries match")?;
            return Ok(());
        }
        let rows: Vec<ListRow> = payload
            .rows
            .iter()
            .map(|r| ListRow {
                key: r.key.chars().take(12).collect(),
                level: r.level.as_str(),
                commit: r
                    .commit
                    .as_deref()
                    .map_or_else(|| "-".to_owned(), |c| c.chars().take(12).collect()),
                series: r.series.clone().unwrap_or_else(|| "-".to_owned()),
                age_days: format!("{}d", r.age_days),
                bytes: r.bytes,
                alias: if r.alias { "yes" } else { "-" }.to_owned(),
            })
            .collect();
        writeln!(w, "{}", styled_table(rows, style))?;
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

fn run_show(
    args: &GlobalArgs,
    caches: &WorkspaceCaches,
    key_prefix: &str,
    full: bool,
) -> CliResult<CommandOutcome> {
    let entries = scan(caches);
    let entry = resolve_prefix(&entries, key_prefix)?;
    let payload = CacheShowPayload {
        key: entry.key.clone(),
        level: entry.level,
        created_at: entry.created_at.clone().unwrap_or_default(),
        age_days: age_days(entry),
        bytes: entry.bytes,
        alias: entry.alias,
        key_inputs: entry.key_inputs.clone(),
        verdict_summary: entry
            .value
            .get("verdict")
            .and_then(|v| v.get("summary"))
            .cloned(),
        value: full.then(|| entry.value.clone()),
    };
    let style = args.table_style;
    let ctx = OutputContext::new(args.formatter, "cache show");
    render(&ctx, &payload, |w| render_show_text(w, &payload, style))?;
    Ok(CommandOutcome::Success)
}

#[derive(Tabled)]
struct FieldRow {
    #[tabled(rename = "FIELD")]
    field: &'static str,
    #[tabled(rename = "VALUE")]
    value: String,
}

#[derive(Tabled)]
struct PinRow {
    #[tabled(rename = "PROJECT")]
    project: String,
    #[tabled(rename = "TAG")]
    tag: String,
    #[tabled(rename = "RESOLVED SHA")]
    resolved: String,
    #[tabled(rename = "SNAPSHOT HASH")]
    snapshot: String,
}

fn render_show_text(
    w: &mut dyn Write,
    payload: &CacheShowPayload,
    style: TableStyle,
) -> CliResult<()> {
    let mut fields = vec![
        FieldRow {
            field: "key",
            value: payload.key.clone(),
        },
        FieldRow {
            field: "level",
            value: payload.level.as_str().to_owned(),
        },
        FieldRow {
            field: "created_at",
            value: payload.created_at.clone(),
        },
        FieldRow {
            field: "age",
            value: format!("{}d", payload.age_days),
        },
        FieldRow {
            field: "bytes",
            value: payload.bytes.to_string(),
        },
    ];
    if payload.alias {
        fields.push(FieldRow {
            field: "alias",
            value: "yes (minted from a content-level hit)".to_owned(),
        });
    }
    if let Some(commit) = payload.key_inputs.get("commit").and_then(|v| v.as_str()) {
        fields.push(FieldRow {
            field: "commit",
            value: commit.to_owned(),
        });
    }
    if let Some(summary) = &payload.verdict_summary {
        fields.push(FieldRow {
            field: "verdict_summary",
            value: serde_json::to_string(summary).unwrap_or_default(),
        });
    }
    writeln!(w, "{}", styled_table(fields, style))?;
    // the pin rows of the pre-image as a nested table, one pin per row
    if let Some(pins) = payload.key_inputs.get("pins").and_then(|v| v.as_array())
        && !pins.is_empty()
    {
        let rows: Vec<PinRow> = pins
            .iter()
            .map(|p| PinRow {
                project: json_str(p, "project"),
                tag: json_str(p, "tag"),
                resolved: json_str(p, "resolved_tag_sha"),
                snapshot: json_str(p, "snapshot_blake3"),
            })
            .collect();
        writeln!(w, "{}", styled_table(rows, style))?;
    }
    if let Some(value) = &payload.value {
        writeln!(
            w,
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        )?;
    }
    Ok(())
}

fn json_str(value: &serde_json::Value, field: &str) -> String {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_owned()
}

fn run_evict(
    args: &GlobalArgs,
    caches: &WorkspaceCaches,
    key_prefixes: &[String],
    commit: Option<&CommitShaPrefix>,
) -> CliResult<CommandOutcome> {
    let entries = scan(caches);
    let mut payload = CacheMutationPayload::default();
    if let Some(commit) = commit {
        for entry in entries.iter().filter(|e| commit_prefix_matches(e, commit)) {
            let removed = evict_entry(entry);
            payload.removed_entries += removed.entries;
            payload.removed_bytes += removed.bytes;
        }
    } else {
        for prefix in key_prefixes {
            let entry = resolve_prefix(&entries, prefix)?;
            let removed = evict_entry(entry);
            payload.removed_entries += removed.entries;
            payload.removed_bytes += removed.bytes;
        }
    }
    render_mutation(args, "cache evict", &payload)
}

fn run_prune(
    args: &GlobalArgs,
    caches: &WorkspaceCaches,
    older_than: u32,
) -> CliResult<CommandOutcome> {
    let removed = prune(
        caches,
        Duration::from_secs(u64::from(older_than) * 24 * 60 * 60),
    );
    let payload = CacheMutationPayload {
        removed_entries: removed.entries,
        removed_bytes: removed.bytes,
    };
    render_mutation(args, "cache prune", &payload)
}

fn run_clear(args: &GlobalArgs, caches: &WorkspaceCaches) -> CliResult<CommandOutcome> {
    let removed = clear(caches);
    let payload = CacheMutationPayload {
        removed_entries: removed.entries,
        removed_bytes: removed.bytes,
    };
    render_mutation(args, "cache clear", &payload)
}

fn render_mutation(
    args: &GlobalArgs,
    command: &'static str,
    payload: &CacheMutationPayload,
) -> CliResult<CommandOutcome> {
    let ctx = OutputContext::new(args.formatter, command);
    render(&ctx, payload, |w| {
        writeln!(
            w,
            "removed {} entr{} ({} bytes)",
            payload.removed_entries,
            if payload.removed_entries == 1 {
                "y"
            } else {
                "ies"
            },
            payload.removed_bytes,
        )?;
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

fn resolve_prefix<'a>(entries: &'a [ScannedEntry], prefix: &str) -> CliResult<&'a ScannedEntry> {
    if prefix.len() < MIN_KEY_PREFIX_LEN || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CliError::InvalidInput(format!(
            "cache-key prefix {prefix:?} must be at least {MIN_KEY_PREFIX_LEN} hex characters"
        )));
    }
    match find_by_key_prefix(entries, prefix) {
        Ok(entry) => Ok(entry),
        Err(KeyLookupError::NotFound) => Err(CliError::CacheKeyNotFound {
            prefix: prefix.to_owned(),
        }),
        Err(KeyLookupError::Ambiguous(candidates)) => Err(CliError::InvalidInput(format!(
            "cache-key prefix {prefix:?} is ambiguous; candidates: {}",
            candidates.join(", ")
        ))),
    }
}

fn age_days(entry: &ScannedEntry) -> u32 {
    u32::try_from(entry.age.as_secs() / (24 * 60 * 60)).unwrap_or(u32::MAX)
}
