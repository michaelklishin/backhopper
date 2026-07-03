# backhopper

`backhopper` is a tool that simplifies dependency compatibility analysis
of git commits (patches) for Erlang- and Elixir-based tools.

Given a set of public API snapshots of dependencies, it can detect whether a patch uses any API
elements that are not available in a specific dependency version.


## What Problem It Solves

When a pull request or a commit (patch) is backported, it has to be
verified against the public API of an older set of dependencies,
ideally before the backporting is done.

Tools such as `xref` require backporting before such verification
can happen.

`backhopper` moves that verification to before the backport. It records
the public API of each dependency at every git tag, then checks any
patch, commit, range, or merge commit against the dependency versions
a target branch uses. Given a checkout of the target branch, it also
checks the patch against the target's own source tree: calls into
first-party modules, `-spec` return shapes, and macro definitions.


## Project Maturity

Young project, breaking changes are likely.


## Binary Releases

Binary releases are available [on the Releases page](https://github.com/michaelklishin/backhopper/releases).


## Build from Source

```shell
cargo install --path crates/backhopper-cli
```

The workspace's binary target is named `backhopper` and installs into `~/.cargo/bin`.


## Usage

### Getting Help

```shell
backhopper help
```

All command groups and individual commands support `--help`:

```shell
backhopper check --help
backhopper check commit --help

backhopper snapshots --help
backhopper snapshots lookup --help
```

Prefixes are inferred when unambiguous, so `backhopper sn li` is the same as
`backhopper snapshots list`.


### Getting Started

Setup is three commands:

```shell
# write a starter backhopper.toml
backhopper init
# see what's configured and what's missing
backhopper doctor
# fill in the missing snapshots
backhopper snapshots generate
```

`init` writes a `backhopper.toml` with an absolute `snapshot_dir` (so it
keeps working when you `cd` away later) and a commented-out
`[[project]]` example to start from.

`doctor` reports workspace health: each series, the pin it expects,
whether a snapshot is on disk, and the newest tag the snapshot store
knows for that project. When something needs fixing, it prints the
exact command to run: `snapshots generate` for a missing snapshot,
`snapshots rebuild` for one written by an older extractor version,
`series sync` for a pin that trails the store after a landed pin bump.
Pass `--check-remote` and it also reports how many new tags each
project has upstream past the latest one you've captured.

Once you have a project or two in `backhopper.toml`, the day-to-day
loop is mostly `backhopper check ...` (covered below). When a patch
references a module you didn't know to track, `--suggest-projects`
prints a copy-pasteable `[[project]]` stub for it.

If you work on RabbitMQ, `init` can also seed itself
from a checkout so you don't have to write `[[series]]` blocks by hand:

```shell
backhopper init --rabbitmq-repo-dir-path /path/to/rabbitmq-server.git
```

That walks the current set of supported branches, reads each
`rabbitmq-components.mk`, and turns the pinned deps into a ready-to-use
config (with `git_url = "TODO"` placeholders you fill in once).

### Global Flags

A handful of flags apply to every subcommand:

* `--config-file-path PATH` (`-c`, env `BACKHOPPER_CONFIG_FILE_PATH`): path to `backhopper.toml`
* `--snapshot-dir-path PATH` (`-s`, env `BACKHOPPER_SNAPSHOT_DIR_PATH`): override the snapshot directory from the config
* `--formatter json|text|markdown|summary|text-summary` (env `BACKHOPPER_FORMATTER`): default `json`. `summary` emits one JSON line per result, `text-summary` one tab-separated row per result; both are for scripts that don't want the full envelope
* `--quiet` (`-q`): drop everything except errors on stderr
* `--verbose` (`-v`): bump log verbosity. `-v` is info, `-vv` is debug, `-vvv` is trace. `RUST_LOG` wins if set
* `--non-interactive` (env `BACKHOPPER_NON_INTERACTIVE_MODE`): turn off progress spinners and anything that would otherwise prompt; use it in CI
* `--table-style modern|borderless|markdown|sharp|ascii|psql|dots`: pick the look of text-mode tables

The `check` subcommands additionally honor `BACKHOPPER_REPO_DIR_PATH`
as the env form of `--repo-dir-path`, for invocations from outside
the repository; the flag wins when both are set.

### Configuration

`backhopper` reads a TOML config file. With no `-c`, it walks up from
the current directory looking for `.backhopper.toml` then
`backhopper.toml` at each level, stopping at the first `.git` boundary.
Falls back to `$XDG_CONFIG_HOME/backhopper/backhopper.toml`. Override
the path with `-c` or the `BACKHOPPER_CONFIG_FILE_PATH` environment
variable.

Each dependency is a "project". Each release line is a named "series"
that pins one tag per project. Example:

```toml
config_version = 1

[defaults]
snapshot_dir = "/path/to/snapshots"

[[project]]
name    = "lib_a"
git_url = "/path/to/lib_a.git"

[[project]]
name    = "lib_b"
git_url = "/path/to/lib_b.git"

[[series]]
name = "stable-3.x"
pins = [
    { project = "lib_a", tag = "v2.0.4" },
    { project = "lib_b", tag = "v1.8.0" },
]
```

A series can also name a working clone of its release branch. `check
cascade` requires it, and the single-commit `check` commands use it as
the default target checkout:

```toml
[[series]]
name = "stable-3.x"
target_repo_dir_path = "/path/to/checkouts/stable-3.x"
pins = [ ... ]
```

The `projects`, `series`, and `config` command groups inspect the loaded
configuration: `projects list`, `series show`, `config validate`, and so
on. `series sync` rebuilds a `[[series]]` stanza from a RabbitMQ
branch's `rabbitmq-components.mk`.


### Tracking Erlang/OTP

`backhopper` has built-in support for Erlang/OTP, which is a
multi-app monorepo of ~35 applications under `lib/`, plus `erts/`. Set
`layout = "erlang_otp"` and sensible defaults apply:

```toml
[[project]]
name    = "otp"
git_url = "/path/to/otp.git"
layout  = "erlang_otp"
```

The defaults, each overridable field by field:

* `app_roots`: `["lib/*", "erts/preloaded"]`
* `exclude_apps`: `odbc`, `snmp`, `ssh`, `tftp`, `ftp`, `wx`, `megaco`,
  `edoc`, `jinterface`, `diameter` (the RabbitMQ `erlang-rpm` exclusion
  set)
* `excluded_subdirs`: `doc`, `example`, `examples`, `test` (matches the
  zero-dependency Erlang RPM from Team RabbitMQ plus CT suites, and a
  few library-specific idiosyncrasies)
* `tag_pattern`: `OTP-*`
* `min_tag`: `OTP-26.0` (filters out older tags; `oldest_tag` is accepted as an alias)
* `exclude_tag_markers`: `-rc`, `-alpha`, `-beta`, `-pre` (skip pre-release
  tags; set to `[]` to include them)

A series can pin OTP via a pattern instead of a literal tag, with two
pins covering the two OTP minors the series supports:

```toml
[[series]]
name = "rabbitmq-4.1"
pins = [
    { project = "ra",     tag = "v2.16.13" },
    { project = "khepri", tag = "v0.17.0" },
    { project = "osiris", tag = "v1.8.6" },
    { project = "otp",    tag_pattern = "OTP-26.*", select = "latest" },
    { project = "otp",    tag_pattern = "OTP-27.*", select = "latest" },
]
```

`select = "latest"` and `select = "oldest"` resolve at check time against
the snapshot store, so a freshly generated `OTP-26.2.6` snapshot is
picked up by the next `check commit` with no config edit.

### Capturing Snapshots

`snapshots generate` walks every git tag of a project, parses the public
API, and writes one text file per tag:

```shell
backhopper snapshots generate --project lib_a
```

Re-running is cheap; only new tags are scanned. To preview what would be
captured without writing anything, add `--dry-run`. To list tags that have
no snapshot on disk yet without touching the store:

```shell
backhopper snapshots list_tags --project lib_a
```


### Looking Up the API at a Tag

For ad-hoc questions ("was this function exported at v1.8.0?"):

```shell
backhopper snapshots lookup --project lib_a --tag v2.0.4 \
                            --mfa some_module:some_function/2

backhopper snapshots modules --project lib_a --tag v2.0.4
backhopper snapshots exports --project lib_a --tag v2.0.4 --module some_module
```

For a branch that has no tag yet, `tree show` reads a module's API
surface straight from a repo ref, in the same canonical format the
snapshots use. Run it twice with two `--ref` values to compare
branches:

```shell
backhopper tree show --repo-dir-path /path/to/lib_a.git --ref main some_module
```


### Diffing the API Between Two Tags

`snapshots project_diff` emits a structured delta across modules,
exports, callbacks, types, headers, and records.

The text format is minimal: just `added` and `removed` prefixes, and
each line is module-qualified:

```shell
backhopper snapshots project_diff --project ra --from v2.15.2 --to v3.1.2
```

produces

```
removed module ra_file_handle
added module ra_kv
removed export ra:start_cluster/2
added export ra:transfer_leadership/3
added callback ra_machine:live_indexes/1
removed type ra_machine:command/0
added type ra_machine:command/1
```

An arity change shows up as one removal and one addition for the same
function (or type).

To diff the dependency pins of two release series:

```shell
backhopper snapshots series_diff --from-series stable-3.x --to-series stable-4.x
```


### Finding When a Symbol Appeared and Disappeared

```shell
backhopper snapshots introduced --project lib_a \
                                --mfa lib_a:some_function/1
```

Walks every stored tag and reports the first and last tag at which each
MFA was present, with the commit SHA from each endpoint snapshot. Pass
`--timeline` to print one row per tag (present or absent), which makes
gaps visible when a symbol was removed and then re-added later.


### Checking a Patch Against a Series

This is the main thing the tool does. Given a commit on a newer branch,
check whether it will still work against a series that pins older
dependency versions:

```shell
backhopper check commit --series stable-3.x \
                        --repo-dir-path /path/to/your_repo.git \
                        1a2b3c4d
```

Short SHAs, tags, or anything `git rev-parse` understands are accepted.
For a one-off question against a single dependency, `--project` plus
`--tag` replace `--series`.

A merge commit has its own subcommand, because `check commit` on a
merge SHA silently diffs against the first parent and can hide the
real change:

```shell
backhopper check merge --series stable-3.x \
                       --repo-dir-path /path/to/your_repo.git \
                       9f8e7d6c
```

For a commit range:

```shell
backhopper check range --series stable-3.x \
                       --repo-dir-path /path/to/your_repo.git \
                       --range v3.0.0..HEAD
```

A raw unified diff also works, either piped in or read from a file:

```shell
git format-patch -1 --stdout HEAD | \
  backhopper check patch --series stable-3.x

backhopper check patch --series stable-3.x /path/to/the.patch
```

To skip cloning, `check pr` resolves a GitHub PR URL via the `gh` CLI:

```shell
backhopper check pr --series stable-3.x \
    https://github.com/owner/repo/pull/123
```

If a pin's snapshot is missing, the check fails with `snapshots
missing`; pass `--auto-generate` to generate it inline instead.


### Checking Many Commits, and Whole Cascades

For many commits at once, `check batch` evaluates each commit against one
or more series in a single invocation, one row per (commit, series)
pair. The commits file holds one SHA prefix per line; blank lines and
`#` comments are skipped, and `-` reads stdin. Merge SHAs are fine on
any line: they evaluate as the first-parent diff, like `check merge`,
and each row reports its `parent_count`:

```shell
backhopper check batch \
    --series stable-3.x,stable-4.x \
    --repo-dir-path /path/to/your_repo.git \
    --commits-file-path candidates.txt
```

A backport round usually applies the same commits to several release
branches in order. `check cascade` runs that as one invocation with one
result matrix: one leg per series, in the given order, each leg checked
against the target checkout its series stanza names (so every series
passed to it must carry `target_repo_dir_path` in the config):

```shell
backhopper check cascade \
    --series stable-4.x,stable-3.x \
    --repo-dir-path /path/to/your_repo.git \
    --commits-file-path candidates.txt
```


### Checking Against the Target Branch Itself

The checks above compare a patch against dependency snapshots. The
target branch's own source can differ from the source branch too, and
`backhopper` can check for that as well. Pass `--target-repo-dir-path`
(a working clone of the branch the commit is being backported to) to
any `check` command and the cross-branch analysis turns on;
`check cascade` gets it from the series config. `--target-ref` selects
a ref other than `HEAD`.

Against the target tree, `backhopper`:

* detects commits that are already present on the target branch, via
  `git cherry-pick -x` trailers and patch-id equivalence
  (`--skip-already-present` turns this off)
* resolves qualified `mod:fun/arity` calls to modules the target tree
  itself provides and reports calls whose target does not exist there
* where both branches declare a `-spec` for such a call, compares the
  declared return shapes and reports drift; calls with no `-spec` on
  either side are counted as unchecked rather than silently passed
* resolves macro usage against the macro definitions the patch can
  actually reach on the target tree

All of these produce non-blocking findings. One blocking check is
opt-in: `--resolve-untracked-modules` looks up each untracked module's
`.erl` in the target checkout and flips the verdict to `Incompatible`
when the file is absent.

When source and target branches keep the same file under different
paths, `[[path_translation]]` stanzas in `backhopper.toml` map them;
`--path-translations-file-path` adds stanzas from an external file of
the same shape.


### Finding Fixes That Should Have Cascaded

`siblings doctor` walks a source branch (default: `main`) and ranks
commits that look like they should have been cherry-picked to a series'
branch but never were: small test-infrastructure fixes whose subjects
match a per-family vocabulary. Fixes that already cascaded are
suppressed via their `git cherry-pick -x` trailers and patch-id
equivalence, so the list stays quiet on a healthy branch:

```shell
backhopper siblings doctor --series stable-4.x \
                           --repo-dir-path /path/to/your_repo.git
```

The window starts at the last release tag reachable from the target
branch (`--since <SHA|TAG>` overrides it). The exit code is `0` for no
candidates and `3` when at least one surfaced; `--explain` adds the
per-factor score breakdown to every row.


### The Verdict Cache

A verdict depends only on its inputs, so `check commit`,
`check merge`, `check batch`, `check cascade`, and `bisect commit`
cache their evaluations under `<snapshot_dir>/.verdict_cache/`. The
cache has two levels: one keyed on the commit SHA plus every
evaluation input (snapshots, resolved pins, config bytes, versions),
and one keyed on the normalized patch plus the macros it can reach.
The second level means a `git cherry-pick -x` that gives the same
patch a new SHA on the next branch still hits. Any input change is a
miss, and a hit renders byte-identically to a fresh run.

Entries expire after `[cache] ttl_days` (default 42; `0` disables).
`--no-cache` skips the cache for one invocation, for both reads and
writes; `BACKHOPPER_NO_CACHE=1` disables it globally. Debug builds
bypass it by default. The `cache` group manages the workspace's
caches:

```shell
backhopper cache stats
backhopper cache list --commit 1a2b3c4d
backhopper cache show <KEY-PREFIX> --full
backhopper cache evict --commit 1a2b3c4d
backhopper cache prune --older-than 14
backhopper cache clear
```

### Bisecting Across a Project's Tags

`bisect commit` walks every stored tag of a project and reports the
newest tag at which the commit's verdict is still `Compatible`, plus the
tag where it flips:

```shell
backhopper bisect commit --project lib_a 1a2b3c4d
```

A clean run prints one row per pinned dependency: the verdict and how
many symbols were checked.

```
compatible: 2, requires_adaptation: 0, incompatible: 0

┌──────────────────┬────────────┬────────┬──────────────────────────────┐
│ pin              │ verdict    │ reason │ detail                       │
├──────────────────┼────────────┼────────┼──────────────────────────────┤
│ lib_a@v2.0.4     │ Compatible │ -      │ 3 tracked symbols referenced │
│ lib_b@v1.8.0     │ Compatible │ -      │ 0 tracked symbols referenced │
└──────────────────┴────────────┴────────┴──────────────────────────────┘
```

`0 tracked symbols referenced` means the patch never touched that
project's API surface, so there was nothing to break. A non-zero
count is the trust signal: `backhopper` actually checked that many
call sites against the pinned tag.


### Verdicts and Exit Codes

Every per-pin verdict is one of four values; the process exit code is
the worst across the series:

| Verdict | Exit | Meaning |
|---|---:|---|
| `Compatible` | `0` | The patch's tracked references all resolve at the pinned tag |
| `RequiresAdaptation` | `3` | Non-blocking findings: deprecated usage, context drift, return-shape drift, or a missing symbol that a later snapshot tag provides (land the dep pin bump first) |
| `Incompatible` | `3` | Blocking findings: missing symbol (at every known tag), arity change, hidden module, missing prereq |
| `Inapplicable` | `0` | The diff touched no analyzable Erlang surface at this pin (docs-only, schema-only, test-only) |

The process exit code is two-valued: `0` when every pin is clean or
has nothing to check, `3` when any pin needs attention. The four-way
verdict split lives in the JSON envelope (`verdict.verdict` per pin
and the `summary` counters); branch on that, not the exit code, to
tell `Inapplicable` from `Compatible (0 tracked symbols referenced)`.
`Inapplicable` means there was nothing to check; `Compatible` with a
zero count means the check ran and found no tracked references.

One `Inapplicable` case still reports useful detail: a commit whose diff
changes a dep pin in the root `rabbitmq-components.mk` reports the
bump in `diagnostics.pin_bumps` — which pin moved, from and to what,
and whether the bumped-to version has a snapshot, with the exact
`snapshots generate` command when it does not (or pass
`--auto-generate` to write it inline).

`--terse` produces one JSON line for shell consumption:

```shell
backhopper check commit --series stable-3.x --terse 1a2b3c4d
# {"summary":"compatible","pins":2,"scope":"source","exit":0}
```


### Explaining a Non-Zero Count

The verdict table reports counts such as `tracked symbols referenced: N`
but not which call sites contributed. `--explain` prints them per
pinned dependency:

```shell
backhopper check commit --series stable-3.x \
                        --repo-dir-path /path/to/your_repo.git \
                        --explain 1a2b3c4d
```

```
tracked call sites per pin:
  lib_b @ v1.8.0
    lib_b:new_function/2
    lib_b:other_function/1
```

When a pin is `Incompatible`, this is the fastest way to see which MFA
in the patch broke against the pinned tag. In JSON output the same
data lands under `data.results.results[*].tracked_ref_details`.

For a `MissingPrereq` finding against a self-pin, `--suggest-prereqs`
runs `git log -S <name>` to suggest a candidate prerequisite commit.
Off by default: slow on large histories.


### Seeing What Was Skipped

The verdict only covers tracked projects. Calls to anything else — the
OTP standard library, third-party libraries you did not configure, or
dynamic dispatch through variables — are left out on purpose, so the
report stays focused on the tracked dependencies. Add
`--show-untracked-calls` to see what was skipped:

```shell
backhopper check commit --series stable-3.x \
                        --repo-dir-path /path/to/your_repo.git \
                        --show-untracked-calls 1a2b3c4d
```

The footer groups skipped items into three categories. All three are
informational only and never change the verdict:

* **Untracked module calls** are calls to modules that no tracked project
  owns. The output suggests a guess at a project name (a module called
  `something_helper` is reported as "project: something?") so you can
  tell whether you forgot to track a dependency or whether the call is
  genuinely outside the tool's scope
* **Untracked records** are `#name{...}` references for records that no
  tracked snapshot declares. Same idea as untracked calls but for the
  record namespace
* **Unanalyzed dynamic dispatch** is `apply/3`, the `spawn` family, and
  `Mod:fun(...)` where the module or function name is a variable. These
  cannot be resolved statically. `backhopper` counts them so they are
  visible rather than silently dropped

Calls to the OTP standard library (`lists`, `gen_server`, `application`,
and so on) are suppressed by default even when `--show-untracked-calls`
is on, because they rarely change the conclusion. Add `--show-otp-calls`
to include them.

`--suggest-projects` groups the untracked calls by inferred project and
emits a ready-to-paste `[[project]]` stub for each candidate;
`--write-suggestions` appends the stubs to the loaded `backhopper.toml`
(each with `git_url = "TODO"`).


### JSON Output

`--formatter json` is the default. Pass `--formatter text` (or set
`BACKHOPPER_FORMATTER=text`) for the human-readable table renderer:

```shell
backhopper --formatter text check commit \
    --series stable-3.x \
    --repo-dir-path /path/to/your_repo.git \
    1a2b3c4d
```

Diagnostics live under `data.diagnostics`, kept separate from
`data.results` so a JSON consumer cannot mistake them for actionable
verdicts.

### Envelope Introspection

Every JSON envelope carries a `schema_version`. When a consumer (the
`backhopper-driver` crate, a script, an agent) reports a version
mismatch, the `schema` group shows what this binary emits and what
changed between versions:

```shell
# what can this binary emit?
backhopper schema supported_envelope_versions

# the embedded JSON schema for one version, and what changed between two
backhopper schema show 12
backhopper schema diff 11 12
```

### Cross-Reference and Suite Selection Queries

Two more command groups are useful around backports. `xref` runs
whole-program cross-reference queries over an Erlang source tree:
`list_callers`, `list_callees`, `list_undefined`, `list_unused_exports`,
`list_deprecated_calls`, `list_module_cycles`, and more. `suites` maps
modified modules and MFAs to the Common Test suites that exercise them
(`list_for_modules`, `list_for_mfas`, `plan`), for picking which suites
to run after a backport.


## Snapshot Staleness in CI

`backhopper snapshots verify --all` is the check to run in CI. It re-parses
every stored snapshot, reports `verified: N, failed: M, stale_extractor: K`,
and exits non-zero on any parse failure. A non-zero `stale_extractor` count
signals snapshots written by an older extractor than the running binary; rerun
`backhopper snapshots rebuild --project <X> --tag <Y>` for each entry.
`backhopper doctor` reports the same staleness per pin, with the exact
`rebuild` command to run. `snapshots verify --coverage` reports every
`[[series]]` pin missing from the snapshot store.


## Subprojects

 * `crates/backhopper-core` is the library: model types, snapshot I/O,
   store, configuration, and the compatibility analyzer
 * `crates/backhopper-git` is the `gix`-based git access layer
 * `crates/backhopper-erlang` is the Erlang source surface extractor
 * `crates/backhopper-erlang-scan` is the dependency-free Erlang lexing
   crate the extractor and analyzer share
 * `crates/backhopper-elixir` is the Elixir source surface extractor
 * `crates/backhopper-cuttlefish` parses Cuttlefish `.schema` files and
   feeds the embedded Erlang fun bodies through the Erlang extractor
 * `crates/backhopper-cache` is the on-disk verdict cache
 * `crates/backhopper-driver` is the typed client for consuming
   `backhopper` JSON envelopes from other Rust programs
 * `crates/backhopper-xref-graph` provides the call-graph primitives
   (vertices, relations, set algebra, transitive closure)
 * `crates/backhopper-xref-reader` turns Erlang source into call-graph
   input
 * `crates/backhopper-xref` is the cross-reference API crate and the
   test-suite selection adapter
 * `crates/backhopper-test-support` holds shared test fixtures and
   builders
 * `crates/backhopper-cli` is the `backhopper` command line tool


## License

This project is double licensed under the MIT License and the Apache License, Version 2.0.

See `LICENSE-APACHE` and `LICENSE-MIT` for details.

SPDX-License-Identifier: Apache-2.0 OR MIT


## Copyright

(c) 2026 Michael S. Klishin and Contributors.
