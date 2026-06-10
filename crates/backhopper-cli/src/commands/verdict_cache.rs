// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-run verdict-cache session: bypass policy, key construction
//! with per-process memos, counter bookkeeping, and the one-line
//! summary.
//!
//! Key-construction failures (an unresolvable pin tag, an unreadable
//! config file) never fail the run: the pair is counted as bypassed
//! and evaluated normally. The standing rule from the design doc: a
//! new evaluation input either joins the key or its flag joins the
//! bypass list — half-keyed is the only forbidden state.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use backhopper_cache::{
    CacheKeyInputs, CacheMode, ContentOutcome, EvaluationShape, InputMissToken, InputOutcome,
    MissToken, PinKeyRow, TargetKeyInputs, VerdictCache, content_hash, hash_bytes, hash_file,
};
use backhopper_core::config::Config;
use backhopper_core::envelope_version::CURRENT_SCHEMA_VERSION;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::{Pin, PinSpec};
use backhopper_core::model::verdict::SeriesEvaluation;
use backhopper_core::store::{ReadOnly, SnapshotStore};
use backhopper_git::GitRepo;

use crate::cli::GlobalArgs;
use crate::commands::context::snapshot_dir;
use crate::commands::target_repo::TargetContext;

// a self-pin tag is already a resolved SHA, never operator input
#[allow(clippy::disallowed_methods)]
fn self_pin_sha(pin: &Pin) -> Option<CommitSha> {
    CommitSha::new(pin.tag.as_str()).ok()
}

/// The macro-environment component of a content key.
#[derive(Debug, Clone)]
pub enum MacroEnv {
    /// The mirror resolver reproduced the evaluation's macro tables.
    Resolved(String),
    /// The mirror could not be exact; the content key degrades to a
    /// per-commit value, a guaranteed miss instead of a false hit.
    Unresolvable,
    /// The evaluation shape consumes no macros (`bisect`).
    Unused,
}

#[derive(Debug, Default)]
struct Counters {
    l1_hits: Cell<u64>,
    l2_hits: Cell<u64>,
    misses: Cell<u64>,
    bypassed: Cell<u64>,
}

/// One command run's view of the verdict cache.
#[derive(Debug)]
pub struct CacheSession {
    cache: Option<VerdictCache>,
    config_blake3: Option<String>,
    snapshot_hashes: RefCell<BTreeMap<PathBuf, Option<String>>>,
    resolved_tags: RefCell<BTreeMap<(ProjectName, TagName), Option<CommitSha>>>,
    counters: Counters,
    reports: bool,
}

/// L1 consultation result, with the bypass case folded in.
#[derive(Debug)]
pub enum SessionLookup<'c> {
    /// The session or this pair is uncacheable; evaluate normally.
    Bypassed,
    Hit(Box<SeriesEvaluation>),
    Miss(Box<SessionMiss<'c>>),
}

/// An L1 miss: carries the key for content-component derivation and
/// the token the L2 consultation consumes.
#[derive(Debug)]
pub struct SessionMiss<'c> {
    token: InputMissToken<'c>,
    pub key: CacheKeyInputs,
}

/// L2 consultation result.
#[derive(Debug)]
pub enum CacheLookupOutcome<'c> {
    Hit(Box<SeriesEvaluation>),
    Miss(SessionStore<'c>),
}

/// Proof of misses on both levels; consumes into one store.
#[derive(Debug)]
#[must_use = "store the evaluation or the pair stays uncached"]
pub struct SessionStore<'c> {
    token: MissToken<'c>,
}

impl SessionStore<'_> {
    pub fn store(self, evaluation: &SeriesEvaluation) {
        self.token.store(evaluation);
    }
}

impl CacheSession {
    /// Open a session for the `check` family. `bypass` covers the
    /// flags whose inputs live outside the key
    /// (`--resolve-untracked-modules`, `--suggest-prereqs`) and the
    /// shapes with no stable content address (patch, pr, range).
    pub fn open(args: &GlobalArgs, cfg: &Config, no_cache: bool, bypass: bool) -> Self {
        if bypass {
            return Self::disabled(false);
        }
        if !CacheMode::resolve(no_cache).is_enabled() {
            return Self::disabled(true);
        }
        let config_blake3 = fs::read(&cfg.config_path)
            .ok()
            .map(|bytes| hash_bytes(&bytes));
        if config_blake3.is_none() {
            tracing::debug!("config bytes unreadable; verdict cache disabled for this run");
            return Self::disabled(true);
        }
        Self {
            cache: Some(VerdictCache::open(
                &snapshot_dir(args, cfg),
                cfg.cache.ttl_days,
            )),
            config_blake3,
            snapshot_hashes: RefCell::new(BTreeMap::new()),
            resolved_tags: RefCell::new(BTreeMap::new()),
            counters: Counters::default(),
            reports: true,
        }
    }

    /// A session that never caches. `reports` controls whether the
    /// summary line is still emitted (true for policy bypasses, false
    /// for shapes the cache does not apply to at all).
    pub fn disabled(reports: bool) -> Self {
        Self {
            cache: None,
            config_blake3: None,
            snapshot_hashes: RefCell::new(BTreeMap::new()),
            resolved_tags: RefCell::new(BTreeMap::new()),
            counters: Counters::default(),
            reports,
        }
    }

    /// Build the L1 key for a `check`-shaped evaluation. `None` (a
    /// bypass) when any component cannot be computed.
    #[allow(clippy::too_many_arguments)]
    pub fn check_key(
        &self,
        cfg: &Config,
        store: &SnapshotStore<ReadOnly>,
        commit: &CommitSha,
        series: Option<&SeriesName>,
        pins: &[Pin],
        pin_specs: &[PinSpec],
        source_pins: &[Option<Pin>],
        target_ctx: Option<&TargetContext>,
    ) -> Option<CacheKeyInputs> {
        let config_blake3 = self.config_blake3.clone()?;
        let mut pin_rows = Vec::with_capacity(pins.len());
        for (idx, pin) in pins.iter().enumerate() {
            let is_self = pin_specs.get(idx).is_some_and(PinSpec::is_self);
            let resolved = self.resolve_pin_tag(cfg, pin, is_self)?;
            pin_rows.push(PinKeyRow {
                project: pin.project.clone(),
                tag: pin.tag.clone(),
                resolved_tag_sha: Some(resolved),
                snapshot_blake3: self.snapshot_hash(store, &pin.project, &pin.tag)?,
            });
        }
        let mut source_rows = Vec::new();
        for pin in source_pins.iter().flatten() {
            // source pins feed snapshot-level spec diffing only; no
            // repo is read at their tag
            source_rows.push(PinKeyRow {
                project: pin.project.clone(),
                tag: pin.tag.clone(),
                resolved_tag_sha: None,
                snapshot_blake3: self.snapshot_hash(store, &pin.project, &pin.tag)?,
            });
        }
        let target = match target_ctx {
            Some(ctx) => Some(TargetKeyInputs {
                resolved_commit: ctx.index.resolved_commit().clone(),
                translations_blake3: content_hash(&ctx.translations).ok()?,
                walk_limit: ctx.walk_limit,
                already_present_enabled: ctx.already_present_enabled,
            }),
            None => None,
        };
        Some(CacheKeyInputs {
            shape: EvaluationShape::Check,
            commit: commit.clone(),
            series: series.cloned(),
            pins: pin_rows,
            source_pins: source_rows,
            target,
            config_blake3,
            schema_version: CURRENT_SCHEMA_VERSION,
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        })
    }

    /// Build the L1 key for one `bisect` tag row: a single pin whose
    /// repo is never read, no macros, no target.
    pub fn bisect_key(
        &self,
        store: &SnapshotStore<ReadOnly>,
        commit: &CommitSha,
        project: &ProjectName,
        tag: &TagName,
    ) -> Option<CacheKeyInputs> {
        let config_blake3 = self.config_blake3.clone()?;
        Some(CacheKeyInputs {
            shape: EvaluationShape::Bisect,
            commit: commit.clone(),
            series: None,
            pins: vec![PinKeyRow {
                project: project.clone(),
                tag: tag.clone(),
                resolved_tag_sha: None,
                snapshot_blake3: self.snapshot_hash(store, project, tag)?,
            }],
            source_pins: Vec::new(),
            target: None,
            config_blake3,
            schema_version: CURRENT_SCHEMA_VERSION,
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        })
    }

    /// Consult L1. A `None` key counts as bypassed.
    pub fn lookup(&self, key: Option<CacheKeyInputs>) -> SessionLookup<'_> {
        let Some(cache) = &self.cache else {
            self.counters.bypassed.set(self.counters.bypassed.get() + 1);
            return SessionLookup::Bypassed;
        };
        let Some(key) = key else {
            self.counters.bypassed.set(self.counters.bypassed.get() + 1);
            return SessionLookup::Bypassed;
        };
        match cache.lookup(&key) {
            InputOutcome::Hit(evaluation) => {
                self.counters.l1_hits.set(self.counters.l1_hits.get() + 1);
                SessionLookup::Hit(evaluation)
            }
            InputOutcome::MissOnInput(token) => {
                SessionLookup::Miss(Box::new(SessionMiss { token, key }))
            }
        }
    }

    /// Consult L2 with the patch and macro-environment components.
    /// On a hit the L1 alias for this SHA was already minted.
    pub fn consult_content<'c>(
        &self,
        miss: SessionMiss<'c>,
        patch_blake3: String,
        macro_env: &MacroEnv,
    ) -> CacheLookupOutcome<'c> {
        let macro_env_blake3 = match macro_env {
            MacroEnv::Resolved(hash) => Some(hash.clone()),
            // unique per commit, so an inexact mirror can never be
            // shared across SHAs
            MacroEnv::Unresolvable => Some(format!("opaque:{}", miss.key.commit)),
            MacroEnv::Unused => None,
        };
        let content = miss.key.content_key(patch_blake3, macro_env_blake3);
        match miss.token.lookup_content(&content) {
            ContentOutcome::Hit(evaluation) => {
                self.counters.l2_hits.set(self.counters.l2_hits.get() + 1);
                CacheLookupOutcome::Hit(evaluation)
            }
            ContentOutcome::Miss(token) => {
                self.counters.misses.set(self.counters.misses.get() + 1);
                CacheLookupOutcome::Miss(SessionStore { token })
            }
        }
    }

    /// Resolve one target pin's tag to a commit, memoized: self-pins
    /// carry the SHA as their tag; external pins resolve in the pin
    /// project's clone.
    fn resolve_pin_tag(&self, cfg: &Config, pin: &Pin, is_self: bool) -> Option<CommitSha> {
        if is_self {
            return self_pin_sha(pin);
        }
        let memo_key = (pin.project.clone(), pin.tag.clone());
        if let Some(cached) = self.resolved_tags.borrow().get(&memo_key) {
            return cached.clone();
        }
        let resolved = cfg
            .project(&pin.project)
            .ok()
            .and_then(|p| p.require_git_url().ok().map(Path::to_path_buf))
            .and_then(|url| GitRepo::open(url).ok())
            .and_then(|repo| repo.resolve_tag(&pin.tag).ok());
        self.resolved_tags
            .borrow_mut()
            .insert(memo_key, resolved.clone());
        resolved
    }

    fn snapshot_hash(
        &self,
        store: &SnapshotStore<ReadOnly>,
        project: &ProjectName,
        tag: &TagName,
    ) -> Option<String> {
        let path = store.snapshot_path(project, tag).ok()?;
        if let Some(cached) = self.snapshot_hashes.borrow().get(&path) {
            return cached.clone();
        }
        let hash = hash_file(&path).ok();
        self.snapshot_hashes.borrow_mut().insert(path, hash.clone());
        hash
    }

    /// The one observability line per run, split by level so cascade
    /// reuse is distinguishable from re-evaluate-and-recache. Absent
    /// from the envelope by design.
    pub fn report(&self) {
        if !self.reports {
            return;
        }
        let l2 = self.counters.l2_hits.get();
        tracing::info!(
            "verdict cache: {} L1 hits, {} L2 hits ({} aliases minted), {} misses, {} bypassed",
            self.counters.l1_hits.get(),
            l2,
            l2,
            self.counters.misses.get(),
            self.counters.bypassed.get(),
        );
    }
}
