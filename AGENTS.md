# Instructions for AI Agents

## Overview

`backhopper` is a Rust CLI that records the public API of Erlang and
Elixir projects across all their git tags into deterministic textual snapshots,
and answers compatibility questions against those snapshots. Its
primary purpose is to remove the manual research step gating RabbitMQ
patch backports across release branches and dependency versions
(`ra`, `khepri`, `osiris`, `cowboy`).

Full design: `~/Development/md/backhopper/design.md`. Read it before
making non-trivial changes.

## Build and Test

```bash
cargo build --workspace --all-features

cargo fmt --all

RUSTFLAGS="-D warnings" cargo nextest run --workspace --all-features
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-features
```

To filter tests with `cargo nextest`:

```bash
cargo nextest run -E "test(test_name)"
```

Coverage gate (Phase 1): `cargo llvm-cov --workspace --lib --tests`
must report at least 90% line coverage on `backhopper-core`,
`backhopper-erlang`, `backhopper-xref-graph`, `backhopper-xref-reader`,
and `backhopper-xref`. The CLI crate is exempt because integration
tests cover it through the process boundary.

## Repository Layout

The repo is a Cargo workspace with multiple crates:

 * `crates/backhopper-core/`: model types, snapshot I/O, store, config,
   compatibility analysis. No `clap`, no `gix`. No I/O policy decisions
 * `crates/backhopper-cache/`: the on-disk caches — `CacheDir`
   (content-addressed entries: BLAKE3 over canonical JSON, freshness
   documents, atomic writes, optional TTL), `VerdictCache` (the
   two-level content-keyed verdict cache with its type-state token
   protocol for lookups and stores, plus L2 SHA aliasing),
   `CacheMode` (decides whether caching is on at all, from
   `--no-cache`, `BACKHOPPER_NO_CACHE`, `BACKHOPPER_FORCE_CACHE`, and
   the debug-build default), the scan, stats, evict, and prune
   surface behind the `cache` verbs, and the marker-gated daily
   sweep. Depends on `backhopper-core` for `SeriesEvaluation` and
   newtypes; gix-free by design — the CLI computes git-derived key
   inputs and passes them in as values
 * `crates/backhopper-git/`: the gix-backed git layer — `GitRepo`,
   PR-commit enumeration, `ResolvedPatchInput` (shared patch resolution
   with `MergePolicy` and `PrCommitPolicy`), first-parent window walks
   plus cherry-pick suppression primitives (`walk.rs`), and the
   `TargetTreeIndex` builder. Depends on `backhopper-core` for
   newtypes; nothing depends on it except the CLI
 * `crates/backhopper-erlang/`: Erlang source surface extractor —
   tokenizer, attribute parser, spec normalizer
 * `crates/backhopper-elixir/`: Elixir extractor (Phase 4; minimal stub
   in earlier phases)
 * `crates/backhopper-cuttlefish/`: Cuttlefish `.schema` parser that
   locates the embedded `fun(...) -> Body end` bodies and feeds them
   through the Erlang extractor
 * `crates/backhopper-xref-graph/`: whole-program call-graph primitives
   (vertices, relations, set algebra, transitive closure). No Erlang
   knowledge
 * `crates/backhopper-xref-reader/`: Erlang source to call-graph
   reader; depends on `backhopper-erlang` and `backhopper-xref-graph`
 * `crates/backhopper-xref/`: cross-reference façade — predefined
   analyses, `XrefDiff`, and the test-suite-selection adapter
 * `crates/backhopper-cli/`: the binary: a `clap`-based parser, command
   dispatch, output formatters
 * `crates/backhopper-driver/`: statically typed Rust driver that
   embeds or shells out to the `backhopper` CLI on a caller's behalf.
   Owns the subprocess, parses the JSON envelope into deserialised
   payloads, and exposes a type-state builder so "required argument
   missing" is a compile-time error
 * `crates/xtask/`: workspace helper binary. `cargo xtask gen-schema`
   regenerates the JSON schema for every supported envelope version
   and either verifies `crates/backhopper-cli/schema/vN.json` matches
   (default) or rewrites it (`--bless`)

Per-crate `tests/` directories use the layout `unit/`, `integration/`,
`proptests/`, `fixtures/`. Each becomes a single test binary under
`cargo nextest`, which keeps link time down because each binary
depends only on its own crate.

## Key Files

### `backhopper-core`

 * `src/lib.rs`: crate root, re-exports
 * `src/errors.rs`: top-level error enum
 * `src/model/names.rs`: newtypes (`ProjectName`, `TagName`, `Mfa`,
   `Arity`, `CommitSha`, etc.) — every domain primitive is its own type
 * `src/model/snapshot.rs`: `Snapshot<S>` with type-state
   (`Unsorted` and `Canonical`)
 * `src/model/verdict.rs`: `Verdict { Compatible | RequiresAdaptation |
   Incompatible | Inapplicable }` and `Reason` enums
 * `src/model/sibling_drift.rs`: `siblings doctor` types — the
   `SiblingCandidate` scoring pipeline (`Unscored`, then `Scored`),
   `Confidence`, `Vocabulary`, and the pure scorer the
   labeled-corpus F1 gate exercises
 * `src/snapshot/format.rs`: canonical writer
 * `src/snapshot/parser.rs`: canonical reader (rejects non-canonical
   input)
 * `src/snapshot/sort.rs`: canonicalization
 * `src/snapshot/spec_normalize.rs`: `-spec` and `-callback` pretty-printer
 * `src/store/fs.rs`: `SnapshotStore<M>` (`ReadOnly` and `Mutable`)
 * `src/config/mod.rs`: `backhopper.toml` schema, `deny_unknown_fields`
 * `src/versions.rs`: `version_cmp` tag ordering
 * `src/compat/patch.rs`: unified-diff parser, `Patch<S>` type-state
   pipeline (`Raw` → `Analyzed` → `Verdicted`)
 * `src/compat/call_sites.rs`: extractor for `mod:fun(...)`,
   `fun mod:fun/N`, `?MACRO`, `#record{}`; one argument scanner
   (`scan_top_level_args`) backs arity counting, arg splitting, and
   shape classification

### `backhopper-erlang`

 * `src/tokenizer.rs`: line-oriented state machine, balances
   `()` `[]` `{}` `''` `""`
 * `src/attributes.rs`: `-module`, `-export`, `-behaviour`, and friends
 * `src/specs.rs`: `-spec`, `-type`, and `-callback` normalizer
 * `src/records.rs`
 * `src/macros.rs`: narrow allowlist (`?MODULE`, `?LINE`,
   `-export`-list string-concat)
 * `src/cond_compile.rs`: `-ifdef`, `-if`, `-elif`, `-else`
 * `src/deprecated.rs`: collapses the four real source forms into one
 * `src/visibility.rs`: `@hidden`, `-doc(hidden)`, `internal_modules`

### `backhopper-xref-graph`

 * `src/state.rs`: sealed `Mode` and `Phase` markers (`Functions`,
   `Modules`, `Building`, `Built`)
 * `src/vertex.rs`, `src/call.rs`: `Vertex`, `FunctionRef`,
   `FunctionSig`, `CallKind`, `CallTarget`
 * `src/relation.rs`: `Relation` and `VertexSet` algebra; transitive
   closure via Tarjan SCC plus DAG reachability
 * `src/graph.rs`: `CallGraph<M, P>`, `ModuleSummary`, `Deprecation`
 * `src/loc.rs`: `Loc`, `Position`, `PathId`, `PathInterner`

### `backhopper-xref-reader`

 * `src/scanner.rs`: byte-level Erlang scanner with position tracking
 * `src/reader/`: `SourceReader` plus the per-concern submodules `scan`
   (`ModuleBuilder` and the top-level scan loop), `attributes` (parses
   `-module`, `-export`, `-import`, `-behaviour`, `-callback`,
   `-deprecated`, and `-on_load`), and `calls` (call-site extraction:
   `m:f(args)`, `f(args)`, `?MODULE:f(args)`, and unresolved variants)
 * `src/application.rs`: `ProjectLayout`, `ApplicationAssignment`
 * `src/model.rs`: `ModuleData`, `CallSite`, `ReadOutput`

### `backhopper-xref`

 * `src/builder.rs`: `XrefBuilder`
 * `src/xref.rs`: `Xref<M>` façade
 * `src/analysis.rs`: predefined analyses (undefined-call,
   exports-not-used, locals-not-used, deprecated-call,
   callers, callees, cycles, behaviour conformance)
 * `src/diff.rs`: `XrefDiff` and `diff_xrefs`
 * `src/suites.rs`: `suites_referencing`, `suites_referencing_mfas`,
   `is_suite_module`
 * `src/result.rs`: typed result structs and `Display` impls

### `backhopper-cli`

 * `src/main.rs`: entry point
 * `src/cli/mod.rs`: `clap` derive parser. Top-level groups:
   `doctor`, `init`, `projects`, `series`, `snapshots`, `check`,
   `bisect`, `cache`, `config`, `shell`, `xref`, `suites`, `schema`,
   `rev`, `siblings`, `version`. `infer_subcommands = true` so
   `backhopper sn li` resolves to `backhopper snapshots list`
 * `src/commands/verdict_cache.rs`: the per-run `CacheSession` — key
   construction with memos, bypass policy, the hit and miss
   counters, and the one-line summary. `src/commands/macro_env.rs`
   mirrors `FileMap`'s include resolution over gix to hash the
   patch-reachable macro slice for the content key
 * `src/cli/{projects,series,snapshots,check,config,shell,xref,suites,tree_source}.rs`:
   per-group argument shapes. Multi-word verbs use per-variant
   `#[command(name = "list_callers")]` to render in snake_case
   (e.g. `xref list_callers`, `snapshots list_tags`,
   `schema supported_envelope_versions`), matching `rabbitmqadmin`.
   Long-form flags stay in clap's default kebab-case
   (`--config-file-path`, `--from-series`). Do not use enum-level
   `#[command(rename_all = "snake_case")]`: it also rewrites the long
   names of inline-defined args in each variant, silently turning
   `--repo-dir-path` into `--repo_dir_path`
 * `src/commands/{projects,series,snapshots,check,config,shell,xref,suites,rabbitmq_components,version,tree_source}.rs`:
   command handlers. `rabbitmq_components` is the RabbitMQ
   `rabbitmq-components.mk` parser (CLI-local, never in `backhopper-core`)
 * `src/commands/mod.rs`: dispatcher (matches `Group::*` to handlers)
 * `src/output.rs`: text and JSON formatter dispatch. JSON envelope is
   `{schema_version, command, data, exit_code}` for every command
 * `src/tables.rs`: `tabled`-based renderers
 * `src/errors.rs`: CLI error type, `ExitCodeProvider` impl

### Testing

 * `tests/unit/`: pure logic, no I/O
 * `tests/integration/`: drives the CLI binary or core crate end-to-end
   against real git repos in `tempfile::TempDir` — no subprocess
   mocking, no trait fakes
 * `tests/proptests/`: `proptest`-based property tests
 * `tests/fixtures/`: ground-truth `*.erl` files paired with
   `*.expected.api.txt`; canned patches; canned snapshots

## Key Dependencies

 * `gix`: pure-Rust git library. Version is exact-pinned (`gix = "=0.X"`,
   no `^`-style range). Bumps are deliberate, tracked in
   `CHANGELOG.md`. We do not use `git2-rs` (libgit2) — the C dependency
   would be a regression from a pure-Rust posture
 * `clap` (`derive`, `env`): CLI parsing. Newtypes implement `FromStr`
   so command arguments parse directly into the right type
 * `serde`, `serde_json`, `toml`: serialization, config
 * `blake3`: content hashing for cache keys (`backhopper-cache`) and
   patch identity (`backhopper-git::walk::patch_id`)
 * `thiserror`: error enums in library crates; `anyhow` only at
   `backhopper-cli`'s `main` boundary
 * `tracing`, `tracing-subscriber` (`env-filter`): logging. ANSI is
   gated by `bel7_cli::should_colorize_stderr` so the layer honors
   `NO_COLOR` and non-TTY destinations
 * `tabled`: text-table rendering. Styling is governed by
   `bel7_cli::TableStyle` (the `--table-style` global flag), not by
   direct `Style::*` calls
 * `bel7-cli` (`tables`, `clap`, `completions`, `progress`, `errors`,
   `serde`): the single ecosystem-wide CLI toolkit. Provides exit-code
   mapping (`ExitCodeProvider`, `ExitCodeExt`, `codes` constants,
   `run_with_exit_code`), shell-completion generation
   (`CompletionShell::detect`, `generate_completions`), table styling
   (`TableStyle`), color decisions (`should_colorize`,
   `should_colorize_stderr`, `print_error`), and progress reporting
   (`InteractiveReporter`, `NonInteractiveReporter`, `QuietReporter`).
   `sysexits` and the `clap_complete` and `clap_complete_nushell` crates
   are pulled in transitively; do not depend on them directly
 * `proptest`: property tests (dev-dependency only)
 * `assert_cmd`, `predicates`, `insta`, `tempfile`: CLI test
   harness (dev-dependencies in `backhopper-cli`)

We deliberately do *not* take `tokio`, `tar`, `walkdir`, `unidiff`,
`rayon`, or `git2-rs`. See design doc §15.

## Target Rust Version

 * Recent stable Rust (`1.95`+). MSRV pinned via the workspace
   `Cargo.toml` `rust-version` field and bumped deliberately

## Rust Code Style

 * Use top-level `use` statements (imports) to fully-qualified names,
   e.g. `use std::fmt::Display` then write `Display`. Never use
   function-local `use` statements
 * Avoid fully-qualified type names as much as possible; always use a
   module-level `use` if there is no ambiguity in the scope
 * Reduce macro use where possible; prefer reducing duplication via
   the type system (generics, traits, type-state). Some duplication is
   acceptable when the alternative is forced indirection
 * Add unit, integration, and property tests under
   `tests/{unit,integration,proptests}/`, never inline in
   implementation files
 * At the end of each task, run `cargo fmt --all`
 * At the end of each task, run `RUSTFLAGS="-D warnings" cargo clippy
   --workspace --all-features` and fix any warnings
 * At the end of each task, run `RUSTFLAGS="-D warnings" cargo nextest
   run --workspace --all-features` and ensure it is clean

## Domain Primitives Are Newtypes

Never let a `String` represent a project name, tag, module name,
function name, commit SHA, or anything else with a domain meaning.
Use the newtypes in `backhopper_core::model::names`. Each implements
`FromStr`, `Display`, and `serde(transparent)`. This means:

 * Clap parses `--mfa cowboy_req:set_resp_header/3` directly into
   `Mfa`, with validation at the CLI boundary instead of three call
   sites down
 * Path-traversal and similar injection bugs are foreclosed at
   newtype construction (e.g. `TagName` rejects `/`, `\`, NUL, control
   characters)
 * Test failures point at the right type, not "expected String got String"

If a new domain primitive is needed, add it as a newtype, not a
`String` alias.

## Type-State Pattern

Four invariants are lifted into the type system. Honor them; don't
work around them with helper functions that erase the state.

 1. Only `Snapshot<state::Canonical>` may be written or parsed. The
    only way to obtain `Canonical` is `Snapshot::<Unsorted>::into_canonical`
    or `Snapshot::parse`. The reader rejects non-canonical input
 2. `Patch<patch_state::{Raw|Analyzed|Verdicted}>` is a one-way
    pipeline. `verdict` exists only on `Verdicted`. Do not add
    methods that bypass the pipeline
 3. `SnapshotStore<ReadOnly>` has no `write` method. Query commands
    take the read-only handle. Do not introduce a `write_unchecked` or
    similar escape hatch
 4. `PinSpec` (`Literal`, `Pattern`, or `SelfRef`) lives in the config
    layer; the compatibility pipeline only sees the resolved `Pin`. The
    only way to obtain a `Pin` from a `PinSpec::Pattern` is
    `PinSpec::resolve(&store)`; for `PinSpec::SelfRef` the CLI resolves
    against `--repo-dir-path` (`PinSpec::resolve` returns
    `ConfigError::SelfPinNeedsRepoDirPath`). Do not introduce a method
    that returns a `Pin` from a pattern or self-ref spec without
    consulting the store or working repo

If a future change needs to relax one of these, propose it in a PR
description, not by adding an `.unwrap_state` method.

## Project Layouts

`ProjectLayout` enumerates three structural shapes that drive snapshot
extraction:

 * `single_app` (default): one git repo with one Erlang source tree
   under `src/`. `scan_paths` (workspace default or project override)
   selects which files get read
 * `multi_app`: a generic monorepo. The user must specify `app_roots`
   explicitly (e.g. an Elixir umbrella). No policy defaults apply
 * `erlang_otp`: structurally `multi_app`, with RabbitMQ-derived
   defaults baked in (the `--without-*` apps from `erlang-rpm` plus
   `excluded_subdirs` covering `doc`, `example`, `examples`, and `test`).
   `layout = "erlang_otp"` alone is the full config for an OTP project

`ProjectLayout::defaults()` returns a `LayoutDefaults` struct that the
config loader applies field-by-field to any unset value. List fields
**replace** the default when specified by the user: we deliberately do
not introduce a delta-merge mode.

## Snapshot Format Invariants

The on-disk snapshot grammar is described in design doc §6. Two rules
matter most:

 * The writer must emit canonical order (modules then headers
   alphabetical; entries within in fixed class order; alphabetical and
   then arity-ordered within a class). The parser must reject
   non-canonical input
 * Specs and callbacks are run through the pretty-printer in
   `snapshot/spec_normalize.rs`. Reformatting the source must not
   produce a snapshot diff. Add proptests when changing the printer

The snapshot file's `# format-version: N` header is the wire-format
version. Bump it only when the on-disk grammar changes; readers reject
unknown values; bumps ship with a `snapshots migrate` command.

## Git Access

Use `gix` exclusively. Never shell out to `git` from production code
(test helpers may, but should prefer `gix`'s write-side APIs to build
fixtures). The git seam is the `backhopper-git` crate: `GitRepo` plus
the `ResolvedPatchInput` patch resolver. `backhopper-core` stays
gix-free so driver consumers never compile `gix`.

We pin `gix` to an exact version and bump it deliberately. Each bump
gets its own commit and CHANGELOG entry so a regression is bisectable.

## Comments

 * Only add very important comments, both in tests and implementation;
   identifier names and the diff are the documentation

 * No multi-line comment blocks. One short line, above the line it
   describes, when needed at all
 * Comments always sit on their own line *above* the code they document.
   Never as a trailing `// ...` after the code on the same line:

   ```rust
   // Right:
   // dropped by min_tag
   "OTP-25.0",

   // Wrong:
   "OTP-25.0",     // dropped by min_tag
   ```
 * Use `:` as the sentence connector inside `//` comments, never ` - `
   or ` — `
 * No comments referencing the current task, fix number, or callers

## Git Instructions

 * Never add yourself to the list of commit co-authors
 * Never mention yourself in commit messages in any way (no
   "Generated by", no AI tool links, etc)
 * Never skip hooks (`--no-verify`) or bypass signing unless the user
   explicitly requests it

## Style Guide

 * Never add full stops to Markdown list items
 * Use `*` for bullets in Markdown files (matches the user's other
   Rust projects)
 * Wrap Rust identifiers (types, methods, traits, modules, crate
   names, paths) in backticks in Markdown prose. `// ...` line
   comments inside code are an exception: backticks read as noise
   there
 * Use "X and Y" in prose, never "X/Y" slash-shorthand. Unit fractions
   (`bytes/edge`), single-concept abbreviations (`I/O`), and paths or
   code (`tests/unit/`, `m:f/a`) are the exceptions
 * CLI subcommand names use snake_case (e.g. `snapshots list_tags`,
   `xref list_callers`), matching `rabbitmqadmin`. Apply this per
   multi-word variant with `#[command(name = "list_tags")]`, never via
   enum-level `#[command(rename_all = "snake_case")]`: the latter also
   rewrites long arg names inside inline-defined variants, silently
   breaking `--repo-dir-path` into `--repo_dir_path`. Long-form flags
   themselves stay in clap's default kebab-case (`--config-file-path`,
   `--from-series`)

## After Completing a Task

### Iterative Reviews

After completing a task, perform up to twenty iterative reviews of
your changes. In every iteration, look for meaningful improvements
that were missed, gaps in test coverage, deviations from the
instructions in this file, places where a `String` should be a
newtype, or places where a runtime check should be a type-state
constraint.

If no meaningful improvements are found for three iterations in a
row, report it and stop iterating.

## Releases

### How to Roll a New Release

Suppose the current development version in the workspace `Cargo.toml`
is `0.N.0` and `CHANGELOG.md` has a `## v0.N.0 (in development)`
section at the top.

 1. Update the changelog: replace `(in development)` with today's
    date, e.g. `(May 7, 2026)`. Make sure all notable changes since
    the previous release are listed under `Enhancements`,
    `Dependency Upgrades`, `Bug Fixes`
 2. Refresh `Cargo.lock`: `cargo update --workspace`. Verify
    `cargo publish --dry-run --locked --allow-dirty -p backhopper-cli`
    passes for each publishable crate
 3. Commit changelog and lockfile changes with the message `0.N.0`
    (just the version number)
 4. Tag the commit: `git tag v0.N.0`
 5. Bump the dev version: set workspace `Cargo.toml` version to
    `0.(N+1).0`
 6. Run `cargo generate-lockfile`
 7. Add a new `## v0.(N+1).0 (in development)` section to
    `CHANGELOG.md` with `No changes yet.` underneath
 8. Commit with the message `Bump dev version`
 9. Push: `git push && git push --tags`
 10. GitHub Actions publishes to crates.io via Trusted Publishing,
     builds release artifacts, and creates the GitHub Release

### GitHub Actions

Three workflows live under `.github/workflows/`:

 * `ci.yaml`: Clippy and `cargo fmt --check` on Ubuntu 22.04 and 24.04;
   `cargo nextest run --cargo-profile ci --workspace --all-features`
   on Ubuntu, macOS, and Windows against stable and beta Rust; `cargo
   audit`; auto-merge for dependabot PRs
 * `release.yml`: validates `CHANGELOG.md` and `Cargo.toml` against the
   `NEXT_RELEASE_VERSION` repo variable, publishes every crate to
   crates.io in dependency order via Trusted Publishing, builds binary
   archives for eight targets, builds deb, rpm, and MSI packages, signs the
   archives with `cosign`, generates SBOMs, generates Homebrew, AUR,
   and Winget manifests, and creates the GitHub Release
 * `verify-packages.yaml`: post-release smoke test of the Debian, RPM,
   and Windows artifacts against a matrix of distros

All three use
[`michaelklishin/rust-build-package-release-action@v3`](https://github.com/michaelklishin/rust-build-package-release-action).

For verifying YAML syntax, use `yq`, Ruby, or Python YAML modules
(whichever is available).

The `NEXT_RELEASE_VERSION` repository variable must match the version
being released for the workflow's validation step to pass.
