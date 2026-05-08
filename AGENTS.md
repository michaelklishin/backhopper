# Instructions for AI Agents

## Overview

`backhopper` is a Rust CLI that records the public API of Erlang/Elixir
projects across all their git tags into deterministic textual snapshots,
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
must report ≥ 90% line coverage on `backhopper-core` and
`backhopper-erlang`. The CLI crate is exempt because integration
tests cover it through the process boundary.

## Repository Layout

The repo is a Cargo workspace with four crates:

 * `crates/backhopper-core/`: model types, snapshot I/O, store, config,
   git access (via `gix`), compatibility analysis. No `clap`. No I/O
   policy decisions
 * `crates/backhopper-erlang/`: Erlang source surface extractor —
   tokenizer, attribute parser, spec normalizer
 * `crates/backhopper-elixir/`: Elixir extractor (Phase 4; minimal stub
   in earlier phases)
 * `crates/backhopper-cli/`: the binary — `clap` parser, command
   dispatch, `--formatter` text/json output

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
   (`Unsorted`/`Canonical`)
 * `src/model/verdict.rs`: `Verdict { Compatible | RequiresAdaptation |
   Incompatible }` and `Reason` enums
 * `src/snapshot/format.rs`: canonical writer
 * `src/snapshot/parser.rs`: canonical reader (rejects non-canonical
   input)
 * `src/snapshot/sort.rs`: canonicalization
 * `src/snapshot/spec_normalize.rs`: `-spec`/`-callback` pretty-printer
 * `src/store/fs.rs`: `SnapshotStore<M>` (`ReadOnly`/`Mutable`)
 * `src/config/mod.rs`: `backhopper.toml` schema, `deny_unknown_fields`
 * `src/git/mod.rs`: `GitRepo` wrapping `gix::Repository`
 * `src/compat/patch.rs`: unified-diff parser, `Patch<S>` type-state
   pipeline (`Raw` → `Analyzed` → `Verdicted`)
 * `src/compat/call_sites.rs`: extractor for `mod:fun(...)`,
   `?MACRO`, `#record{}`

### `backhopper-erlang`

 * `src/tokenizer.rs`: line-oriented state machine, balances
   `()` `[]` `{}` `''` `""`
 * `src/attributes.rs`: `-module` / `-export` / `-behaviour` / etc.
 * `src/specs.rs`: `-spec` / `-type` / `-callback` normalizer
 * `src/records.rs`
 * `src/macros.rs`: narrow allowlist (`?MODULE`, `?LINE`,
   `-export`-list string-concat)
 * `src/cond_compile.rs`: `-ifdef` / `-if` / `-elif` / `-else`
 * `src/deprecated.rs`: collapses the four real source forms into one
 * `src/visibility.rs`: `@hidden`, `-doc(hidden)`, `internal_modules`

### `backhopper-cli`

 * `src/main.rs`: entry point
 * `src/cli/mod.rs`: `clap`-based parser, command groups
 * `src/cli/dispatch.rs`: command dispatching
 * `src/commands/{projects,series,snapshots,api,compatibility,config,completions,version}.rs`
 * `src/output.rs`: text/json formatter dispatch
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
 * `clap_complete`, `clap_complete_nushell`: shell completions
 * `serde`, `serde_json`, `toml`: serialization, config
 * `thiserror`: error enums in library crates; `anyhow` only at
   `backhopper-cli`'s `main` boundary
 * `tracing`, `tracing-subscriber` (`env-filter`): logging
 * `tabled`, `owo-colors`: text output
 * `bel7-cli`: exit-code mapping, table styles, completion glue
 * `sysexits`: exit-code constants
 * `proptest`: property tests (dev-dependency only)
 * `assert_cmd`, `predicates`, `insta`, `tempfile`: CLI test
   harness (dev-dependencies in `backhopper-cli`)

We deliberately do *not* take `tokio`, `tar`, `walkdir`, `unidiff`,
`rayon`, or `git2-rs`. See design doc §15.

## Target Rust Version

 * Recent stable Rust (e.g. `1.94`+). MSRV pinned via
   `rust-toolchain.toml` and bumped deliberately

## Rust Code Style

 * Use top-level `use` statements (imports) to fully-qualified names,
   e.g. `use std::fmt::Display` then write `Display`. Never use
   function-local `use` statements
 * Add unit/integration/property tests under `tests/{unit,integration,proptests}/`,
   never inline in implementation files
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

Three invariants are lifted into the type system. Honor them; don't
work around them with helper functions that erase the state.

 1. Only `Snapshot<state::Canonical>` may be written or parsed. The
    only way to obtain `Canonical` is `Snapshot::<Unsorted>::into_canonical`
    or `Snapshot::parse`. The reader rejects non-canonical input
 2. `Patch<patch_state::{Raw|Analyzed|Verdicted}>` is a one-way
    pipeline. `verdict()` exists only on `Verdicted`. Do not add
    methods that bypass the pipeline
 3. `SnapshotStore<ReadOnly>` has no `write` method. Query commands
    take the read-only handle. Do not introduce a `write_unchecked` or
    similar escape hatch

If a future change needs to relax one of these, propose it in a PR
description, not by adding a `.unwrap_state()` method.

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
fixtures). The git seam is the single `GitRepo` struct in
`backhopper_core::git`.

We pin `gix` to an exact version and bump it deliberately. Each bump
gets its own commit and CHANGELOG entry so a regression is bisectable.

## Comments

 * Only add very important comments, both in tests and implementation.
   Identifier names and the diff are the documentation
 * No multi-line comment blocks. One short line, above the line it
   describes, when needed at all
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

The release workflow uses
[`michaelklishin/rust-build-package-release-action`](https://github.com/michaelklishin/rust-build-package-release-action).

For verifying YAML syntax, use `yq`, Ruby, or Python YAML modules
(whichever is available).

The `NEXT_RELEASE_VERSION` repository variable must match the version
being released for the workflow's validation step to pass.
