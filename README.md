# backhopper

`backhopper` is a tool that simplifies dependency compatibility analysis
of git commits (patches) for Erlang- and Elixir-based tools.

Given a set of public API snapshots of dependencies, it can detect whether a patch uses any API
elements that are not available in a specific dependency version.


## Table of Contents

| Task | Section |
|---|---|
| First-time setup | [Getting Started](#getting-started) |
| Capture or refresh dependency snapshots | [Capturing Snapshots](#capturing-snapshots) |
| Track Erlang/OTP as a dependency | [Tracking Erlang/OTP](#tracking-erlangotp) |
| Ask what an API looked like at a tag | [Looking Up the API at a Tag](#looking-up-the-api-at-a-tag) |
| Compare two tags or two series | [Diffing the API Between Two Tags](#diffing-the-api-between-two-tags) |
| Check a commit, merge, range, patch, or PR | [Checking a Patch Against a Series](#checking-a-patch-against-a-series) |
| Check many commits or several series at once | [Checking Many Commits, and Whole Cascades](#checking-many-commits-and-whole-cascades) |
| Compare against the target branch's own source | [Checking Against the Target Branch Itself](#checking-against-the-target-branch-itself) |
| Check Elixir sources | [Elixir Sources](#elixir-sources) |
| Find fixes that never got backported | [Finding Fixes That Should Have Cascaded](#finding-fixes-that-should-have-cascaded) |
| Run a whole backport round | [A Full Backport Round](#a-full-backport-round) |
| Decode an error message | [Common Errors](#common-errors) |
| Find the tag where a commit's verdict flips | [Bisecting Across a Project's Tags](#bisecting-across-a-projects-tags) |
| Interpret verdicts and exit codes | [Verdicts and Exit Codes](#verdicts-and-exit-codes) |
| Inspect or clear the verdict cache | [The Verdict Cache](#the-verdict-cache) |
| Consume results from scripts | [JSON Output](#json-output) |
| Embed the tool in a Rust program | [Driving backhopper from Rust](#driving-backhopper-from-rust) |
| Automate in CI | [Running in CI](#running-in-ci) |


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


## How It Works, and What It Does Not Do

`backhopper` is static analysis over a diff. It extracts the references
a patch adds — qualified `m:f/a` calls, local calls, records, macros,
types, behaviours — and resolves each one against two sources: per-tag
API snapshots of the pinned dependencies, and, when a target checkout
is available, the target branch's own source tree.

`xref` and `dialyzer` answer related questions from source: both need
the complete codebase, with dependencies fetched and the backport
already applied. `backhopper` works from pre-generated dependency API
snapshots, so the dependencies never have to be present in the source
tree and the question is answered before the backport exists. The
tools sit at the two ends of a backporting workflow: `backhopper`
suggests what can or cannot be backported safely without adaptation,
`xref` and `dialyzer` test the result after the backporting is done.

It does not compile or run anything. That sets the limits:

* `Compatible` means every extracted reference resolves at the pinned
  versions; it says nothing about runtime behavior
* dynamic dispatch (`apply/3`, `Mod:fun(...)` with a variable module or
  function name) cannot be resolved statically; it is counted and
  reported, never checked
* return shapes are compared only where both branches declare a
  `-spec`; deeper return-type divergence is left to `dialyzer`
* only the diff is analyzed: code the patch does not touch is out of
  scope


## An End-to-End Example

Track one dependency, snapshot it, check a commit against a release
series. A series names a release line; each pin is the dependency tag
that line uses. Every command shown here has a dedicated section under
[Usage](#usage) below, and [Binary Releases](#binary-releases) covers
installation.

Shell examples in this document are written for POSIX shells.
Single-line commands run unchanged in Nu shell; where a command needs
Nu-specific quoting or layout (no backslash continuation, `..` is
range syntax), a Nu version follows it.

```toml
# backhopper.toml
config_version = 1

[defaults]
snapshot_dir = "/path/to/snapshots"

[[project]]
name    = "ra"
git_url = "https://github.com/rabbitmq/ra.git"

[[series]]
name = "rabbitmq-4.2"
pins = [{ project = "ra", tag = "v2.17.1" }]
```

```shell
backhopper snapshots generate --project ra

# this commit SHA is an example meant to be replaced with an actual SHA
backhopper --formatter text check commit --series rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git 1a2b3c4d
```

The same example with Nu shell:

```nu
backhopper snapshots generate --project ra

# this commit SHA is an example meant to be replaced with an actual SHA
backhopper --formatter text check commit --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git 1a2b3c4d
```

```
compatible: 0, requires_adaptation: 1, incompatible: 0

┌────────────┬────────────────────┬───────────────┬──────────────────────┐
│ pin        │ verdict            │ reason        │ detail               │
├────────────┼────────────────────┼───────────────┼──────────────────────┤
│ ra@v2.17.1 │ RequiresAdaptation │ MissingSymbol │ ra:member_overview/2 │
└────────────┴────────────────────┴───────────────┴──────────────────────┘
```

The commit calls `ra:member_overview/2`, which does not exist at the
pinned `v2.17.1` but does at a later `ra` tag, so the verdict is
`RequiresAdaptation` rather than `Incompatible`: land the pin bump on
the target branch first, then backport. The process exits with code
`3`. `snapshots introduced --project ra --mfa ra:member_overview/2`
reports the tag that added the symbol.


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

The examples below track three RabbitMQ dependencies — `ra`, `khepri`,
and `osiris` — across two release series. Nothing about the tool is
RabbitMQ-specific: any Erlang or Elixir project with git-tagged
dependencies works the same way.


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
name    = "ra"
git_url = "https://github.com/rabbitmq/ra.git"

[[project]]
name    = "khepri"
git_url = "https://github.com/rabbitmq/khepri.git"

[[project]]
name    = "osiris"
git_url = "https://github.com/rabbitmq/osiris.git"

[[series]]
name = "rabbitmq-4.2"
pins = [
    { project = "ra",     tag = "v2.17.1" },
    { project = "khepri", tag = "v0.17.1" },
    { project = "osiris", tag = "v1.9.0" },
]
```

A series can also name a working clone of its release branch. `check
cascade` requires it, and the single-commit `check` commands use it as
the default target checkout:

```toml
[[series]]
name = "rabbitmq-4.2"
target_repo_dir_path = "/path/to/checkouts/v4.2.x"
pins = [ ... ]
```

The `projects`, `series`, and `config` command groups inspect the loaded
configuration: `projects list`, `series show`, `config validate`, and so
on.

`series sync` rebuilds a `[[series]]` stanza from a RabbitMQ
branch's `rabbitmq-components.mk` dependency list.


### Tracking Erlang/OTP

`backhopper` has built-in support for Erlang/OTP, which is a
multi-app monorepo of ~35 applications under `lib/`, plus `erts/`.

Use `layout = "erlang_otp"` for Erlang/OTP dependencies

```toml
[[project]]
name    = "otp"
git_url = "/path/to/otp.git"
layout  = "erlang_otp"
```

The defaults, each overridable field by field:

* `app_roots`: `["lib/*", "erts/preloaded"]`
* `exclude_apps`: `odbc`, `snmp`, `ssh`, `tftp`, `ftp`, `wx`, `megaco`,
  `edoc`, `jinterface`, `diameter` (RabbitMQ does not use these, Team RabbitMQ's
  zero dependency RPM excludes the same list of apps)
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
name = "rabbitmq-4.2"
pins = [
    { project = "ra",     tag = "v2.17.1" },
    { project = "khepri", tag = "v0.17.1" },
    { project = "osiris", tag = "v1.9.0" },
    { project = "otp",    tag_pattern = "OTP-27.*", select = "latest" },
    { project = "otp",    tag_pattern = "OTP-28.*", select = "latest" },
]
```

`select = "latest"` and `select = "oldest"` resolve at check time against
the snapshot store, so a freshly generated `OTP-27.3.5` snapshot is
picked up by the next `check commit` with no config edit.

### Capturing Snapshots

`snapshots generate` walks every git tag of a project, parses the public
API, and writes one text file per tag:

```shell
backhopper snapshots generate --project ra
```

A snapshot is a plain text file: a commented header naming the project,
tag, commit, and extractor version, then one block per module, all in a
canonical order so two scans of the same tree are byte-identical. An
excerpt:

```
# backhopper snapshot
# format-version: 4
# project: ra
# tag: v2.17.1
# commit: 4f0c2b8e1d9a...
# extractor-version: 5

module ra_machine
  path src/ra_machine.erl
  export init/1
  export apply/3
  callback live_indexes/1
  type command/1 :: term()
```

The files are diffable and safe to commit; the lookup, diff, and check
commands read them, not the project's git history.

Re-running is cheap; only new tags are scanned. To preview what would be
captured without writing anything, add `--dry-run`. To list tags that have
no snapshot on disk yet without touching the store:

```shell
backhopper snapshots list_tags --project ra
```


### Looking Up the API at a Tag

For ad-hoc questions ("was this function exported at v2.17.1?"):

```shell
backhopper snapshots lookup --project ra --tag v2.17.1 \
                            --mfa ra:transfer_leadership/3

backhopper snapshots modules --project ra --tag v2.17.1
backhopper snapshots exports --project ra --tag v2.17.1 --module ra_machine
```

The same example with Nu shell:

```nu
backhopper snapshots lookup --project ra --tag v2.17.1 --mfa ra:transfer_leadership/3

backhopper snapshots modules --project ra --tag v2.17.1
backhopper snapshots exports --project ra --tag v2.17.1 --module ra_machine
```

For a branch that has no tag yet, `tree show` reads a module's API
surface straight from a repo ref, in the same canonical format the
snapshots use. Run it twice with two `--ref` values to compare
branches:

```shell
backhopper tree show --repo-dir-path /path/to/ra.git --ref main ra_server
```


### Diffing the API Between Two Tags

`snapshots project_diff` emits a structured delta across modules,
exports, callbacks, types, headers, and records.

The text format is minimal: just `added` and `removed` prefixes, and
each line is module-qualified:

```shell
backhopper snapshots project_diff --project ra --from v2.17.1 --to v3.1.2
```

produces

```
removed module ra_file_handle
added module ra_kv
removed export ra:start_cluster/2
added export ra:member_overview/2
added callback ra_machine:live_indexes/1
removed type ra_machine:command/0
added type ra_machine:command/1
```

An arity change shows up as one removal and one addition for the same
function (or type).

To diff the dependency pins of two release series:

```shell
backhopper snapshots series_diff --from-series rabbitmq-4.2 --to-series rabbitmq-4.3
```


### Finding When a Symbol Appeared and Disappeared

```shell
backhopper snapshots introduced --project ra \
                                --mfa ra:key_metrics/1
```

The same example with Nu shell:

```nu
backhopper snapshots introduced --project ra --mfa ra:key_metrics/1
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
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 \
                        --repo-dir-path /path/to/rabbitmq-server.git \
                        1a2b3c4d
```

The same example with Nu shell:

```nu
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git 1a2b3c4d
```

Short SHAs, tags, or anything `git rev-parse` understands are accepted.
For a one-off question against a single dependency, `--project` plus
`--tag` replace `--series`.

With the text formatter, a clean run prints one row per pinned
dependency: the verdict and how many symbols were checked.

```
compatible: 3, requires_adaptation: 0, incompatible: 0

┌────────────────┬────────────┬────────┬──────────────────────────────┐
│ pin            │ verdict    │ reason │ detail                       │
├────────────────┼────────────┼────────┼──────────────────────────────┤
│ ra@v2.17.1     │ Compatible │ -      │ 3 tracked symbols referenced │
│ khepri@v0.17.1 │ Compatible │ -      │ 0 tracked symbols referenced │
│ osiris@v1.9.0  │ Compatible │ -      │ 0 tracked symbols referenced │
└────────────────┴────────────┴────────┴──────────────────────────────┘
```

`0 tracked symbols referenced` means the patch never touched that
project's API surface, so there was nothing to break. A non-zero
count is the trust signal: `backhopper` actually checked that many
call sites against the pinned tag.

A merge commit has its own subcommand, because `check commit` on a
merge SHA silently diffs against the first parent and can hide the
real change:

```shell
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check merge --series rabbitmq-4.2 \
                       --repo-dir-path /path/to/rabbitmq-server.git \
                       9f8e7d6c
```

The same example with Nu shell:

```nu
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check merge --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git 9f8e7d6c
```

For a commit range:

```shell
backhopper check range --series rabbitmq-4.2 \
                       --repo-dir-path /path/to/rabbitmq-server.git \
                       --range v4.2.0..HEAD
```

The same example with Nu shell; `..` is range syntax in Nu, so the
value needs quotes:

```nu
backhopper check range --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git --range "v4.2.0..HEAD"
```

A raw unified diff also works, either piped in or read from a file:

```shell
git format-patch -1 --stdout HEAD | \
  backhopper check patch --series rabbitmq-4.2

backhopper check patch --series rabbitmq-4.2 /path/to/the.patch
```

The same example with Nu shell:

```nu
git format-patch -1 --stdout HEAD | backhopper check patch --series rabbitmq-4.2

backhopper check patch --series rabbitmq-4.2 /path/to/the.patch
```

To skip cloning, `check pr` resolves a GitHub PR URL via the `gh` CLI:

```shell
backhopper check pr --series rabbitmq-4.2 \
    https://github.com/rabbitmq/rabbitmq-server/pull/12345
```

The same example with Nu shell:

```nu
backhopper check pr --series rabbitmq-4.2 https://github.com/rabbitmq/rabbitmq-server/pull/12345
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
    --series rabbitmq-4.3,rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git \
    --commits-file-path candidates.txt
```

The same example with Nu shell:

```nu
backhopper check batch --series "rabbitmq-4.3,rabbitmq-4.2" --repo-dir-path /path/to/rabbitmq-server.git --commits-file-path candidates.txt
```

A backport round usually applies the same commits to several release
branches in order. `check cascade` runs that as one invocation with one
result matrix: one leg per series, in the given order, each leg checked
against the target checkout its series stanza names (so every series
passed to it must carry `target_repo_dir_path` in the config):

```shell
backhopper check cascade \
    --series rabbitmq-4.3,rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git \
    --commits-file-path candidates.txt
```

The same example with Nu shell:

```nu
backhopper check cascade --series "rabbitmq-4.3,rabbitmq-4.2" --repo-dir-path /path/to/rabbitmq-server.git --commits-file-path candidates.txt
```

The text output is a matrix, one row per commit and one verdict column
per leg, followed by one block per leg with the clearance and the
resolution tallies (trimmed here to one leg):

```
sha          subject                          rabbitmq-4.3          rabbitmq-4.2
1a2b3c4d90   Handle coordinator timeout       compatible            requires_adaptation!
9f8e7d6c21   CQ: fix shared store leak        inapplicable          inapplicable
! symbol findings on this leg

leg rabbitmq-4.2 (/path/to/checkouts/v4.2.x at HEAD):
clearance: 2 candidates × 1 series · exit=NEEDS_ATTENTION
  tracked dep surface : 3 symbols referenced
  verdicts            : compatible=0 requires_adaptation=1 incompatible=0 inapplicable=1
```

A `*` marker beside a verdict flags a predicted apply conflict on that
leg, `!` flags symbol findings against the leg's target tree.


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

```shell
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper --formatter text check commit --series rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git \
    --target-repo-dir-path /path/to/checkouts/v4.2.x \
    1a2b3c4d
```

The same example with Nu shell:

```nu
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper --formatter text check commit --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git --target-repo-dir-path /path/to/checkouts/v4.2.x 1a2b3c4d
```

Each finding names the symbol and its position in the patch:

```
rabbit_stream_coordinator:transfer_leadership/2 undefined on target (deps/rabbit/src/rabbit_stream_queue.erl:212)
rabbit_fifo_client:enqueue/3: source spec "ok | {reject_publish, term()}" vs target spec "ok" (deps/rabbit/src/rabbit_channel.erl:641)
```

All of these produce non-blocking findings. One blocking check is
opt-in: `--resolve-untracked-modules` looks up each untracked module's
`.erl` in the target checkout and flips the verdict to `Incompatible`
when the file is absent.

When source and target branches keep the same file under different
paths, `[[path_translation]]` stanzas in `backhopper.toml` map them;
`--path-translations-file-path` adds stanzas from an external file of
the same shape.


### Elixir Sources

`.ex` files in the diff are analyzed alongside `.erl` files, and Elixir
projects can be tracked and snapshotted like Erlang ones. Elixir code
that calls into Erlang modules — the `rabbitmq_cli` pattern — resolves
against the same snapshots and target tree. A CLI command that reaches
a broker module over rpc:

```elixir
:rabbit_misc.rpc_call(node_name, :rabbit_quorum_queue, :shrink_all, [node_name])
```

resolves `rabbit_quorum_queue:shrink_all/1` like a qualified Erlang
call and reports it when the target branch does not define it:

```
rabbit_quorum_queue:shrink_all/1 undefined on target (via rabbit_misc:rpc_call, deps/rabbitmq_cli/lib/rabbitmq/cli/queues/commands/shrink_command.ex:38)
```

A diff that touches only `.ex` files is analyzable surface like any
other, not `Inapplicable`.


### Finding Fixes That Should Have Cascaded

`siblings doctor` walks a source branch (default: `main`) and ranks
commits that look like they should have been cherry-picked to a series'
branch but never were: small test-infrastructure fixes whose subjects
match a per-family vocabulary. Fixes that already cascaded are
suppressed via their `git cherry-pick -x` trailers and patch-id
equivalence, so the list stays quiet on a healthy branch:

```shell
backhopper siblings doctor --series rabbitmq-4.2 \
                           --repo-dir-path /path/to/rabbitmq-server.git
```

The same example with Nu shell:

```nu
backhopper siblings doctor --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git
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
# the commit SHA is an example meant to be replaced with an actual SHA
backhopper cache stats
backhopper cache list --commit 1a2b3c4d
backhopper cache show <KEY-PREFIX> --full
backhopper cache evict --commit 1a2b3c4d
backhopper cache prune --older-than 14
backhopper cache clear
```

The same commands run unchanged in Nu shell, except that `<` starts a
redirection there, so quote the key prefix argument:

```nu
backhopper cache show "<KEY-PREFIX>" --full
```

### Bisecting Across a Project's Tags

`bisect commit` walks every stored tag of a project and reports the
newest tag at which the commit's verdict is still `Compatible`, plus the
tag where it flips:

```shell
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper bisect commit --project ra 1a2b3c4d
```


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
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 --terse 1a2b3c4d
# {"summary":"compatible","pins":3,"scope":"source","exit":0}
```


### Explaining a Non-Zero Count

The verdict table reports counts such as `tracked symbols referenced: N`
but not which call sites contributed. `--explain` prints them per
pinned dependency:

```shell
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 \
                        --repo-dir-path /path/to/rabbitmq-server.git \
                        --explain 1a2b3c4d
```

The same example with Nu shell:

```nu
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git --explain 1a2b3c4d
```

```
tracked call sites per pin:
  ra @ v2.17.1
    ra:transfer_leadership/3
    ra:key_metrics/1
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
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 \
                        --repo-dir-path /path/to/rabbitmq-server.git \
                        --show-untracked-calls 1a2b3c4d
```

The same example with Nu shell:

```nu
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper check commit --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git --show-untracked-calls 1a2b3c4d
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
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper --formatter text check commit \
    --series rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git \
    1a2b3c4d
```

The same example with Nu shell:

```nu
# this commit SHA is an example meant to be replaced with an actual SHA
backhopper --formatter text check commit --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git 1a2b3c4d
```

Every response is one envelope: `schema_version`, `command`,
`exit_code`, and a verb-specific `data`. A trimmed `check commit`
envelope:

```json
{
  "schema_version": 15,
  "command": "check commit",
  "exit_code": 3,
  "data": {
    "queried_against": { "kind": "series", "name": "rabbitmq-4.2", "pins": ["..."] },
    "results": {
      "results": [
        {
          "pin": { "project": "ra", "tag": "v2.17.1" },
          "verdict": {
            "verdict": "requires_adaptation",
            "reasons": [{ "kind": "missing_symbol", "...": "..." }]
          },
          "tracked_refs": 3,
          "tracked_ref_details": ["..."]
        }
      ],
      "summary": {
        "compatible": 0,
        "requires_adaptation": 1,
        "incompatible": 0,
        "inapplicable": 0
      }
    },
    "diagnostics": { "...": "..." }
  }
}
```

Scripts branch on `data.results.results[*].verdict.verdict` and the
`data.results.summary` counters. Diagnostics live under
`data.diagnostics`, kept separate from `data.results` so a JSON
consumer cannot mistake them for actionable verdicts.

### Envelope Introspection

Every JSON envelope carries a `schema_version`. When a consumer (the
`backhopper-driver` crate, a script, an agent) reports a version
mismatch, the `schema` group shows what this binary emits and what
changed between versions:

```shell
# what can this binary emit?
backhopper schema supported_envelope_versions

# the embedded JSON schema for one version, and what changed between two
backhopper schema show 15
backhopper schema diff 14 15
```

### Driving backhopper from Rust

The `backhopper-driver` crate is a typed client for programs that embed
`backhopper` rather than shell out to it. It discovers and owns the
subprocess, parses the JSON envelope into typed payloads, and
checks the envelope's `schema_version` against the range the crate
understands. The builders use type-state, so a missing required
argument is a compile error rather than a runtime one, and every
failure path is a structured `DriverError` variant (binary not found,
spawn failure, timeout, schema mismatch, and so on) instead of an exit
code to interpret:

```rust
use backhopper_driver::Backhopper;
use backhopper_driver::types::SeriesName;
use std::str::FromStr;

let driver = Backhopper::auto_discover()?;
let series = SeriesName::from_str("rabbitmq-4.2")?;

let evaluation = driver.check()
    .patch()
    .series(series)
    .patch_bytes(std::fs::read("pr-12345.patch")?)
    .run()?;

println!("verdict: {:?}", evaluation.worst_verdict());
```

`run_with_diagnostics()` additionally returns the `ExecutedInvocation`:
the exact argv, exit code, and wall-clock duration, for logging or
replay.

### Cross-Reference and Suite Selection Queries

Two more command groups are useful around backports. `xref` runs
whole-program cross-reference queries over an Erlang source tree:
`list_callers`, `list_callees`, `list_undefined`, `list_unused_exports`,
`list_deprecated_calls`, `list_module_cycles`, and more. `suites` maps
modified modules and MFAs to the Common Test suites that exercise them
(`list_for_modules`, `list_for_mfas`, `plan`), for picking which suites
to run after a backport.

### A Full Backport Round

The commands compose into a round. Screen the candidate commits against
every maintained series, each series checked against its own target
checkout:

```shell
backhopper check cascade --series rabbitmq-4.3,rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git \
    --commits-file-path candidates.txt
```

Check whether earlier fixes should ride along:

```shell
backhopper siblings doctor --series rabbitmq-4.2 \
    --repo-dir-path /path/to/rabbitmq-server.git
```

After the picks land on the target branch, select the Common Test
suites to run:

```shell
git -C /path/to/checkouts/v4.2.x diff --name-only v4.2.1..HEAD | \
  backhopper suites plan --repo-dir-path /path/to/checkouts/v4.2.x \
      --modified-paths-file-path -
```

The same round with Nu shell; a pipeline continues across lines after
a trailing `|`, and the git range needs quotes:

```nu
backhopper check cascade --series "rabbitmq-4.3,rabbitmq-4.2" --repo-dir-path /path/to/rabbitmq-server.git --commits-file-path candidates.txt

backhopper siblings doctor --series rabbitmq-4.2 --repo-dir-path /path/to/rabbitmq-server.git

git -C /path/to/checkouts/v4.2.x diff --name-only "v4.2.1..HEAD" |
  backhopper suites plan --repo-dir-path /path/to/checkouts/v4.2.x --modified-paths-file-path -
```

### Common Errors

| Message | Meaning | Fix |
|---|---|---|
| `snapshots missing` | a pin in the series has no snapshot on disk | `snapshots generate --project <NAME>`, or re-run the check with `--auto-generate` |
| `commit 1a2b3c4d not found in repository ...: did you forget to git fetch?` | the SHA is not reachable in the clone `--repo-dir-path` points at | fetch the branch, or point at the clone that has it |
| `STALE` rows in `doctor` output | a snapshot was written by an older extractor version than the running binary | `snapshots rebuild --project <NAME> --tag <TAG>` |
| a pin listed by `snapshots verify --coverage` | a `[[series]]` pin has no snapshot in the store | `snapshots generate --project <NAME>` |


## Running in CI

Two checks are worth automating: the snapshot store is healthy, and the
change under review passes against every maintained series. In GitHub
Actions terms:

```yaml
- name: Verify snapshots
  run: backhopper --non-interactive snapshots verify --all

- name: Check the PR against the 4.2 series
  run: |
    backhopper --non-interactive check pr --series rabbitmq-4.2 \
        "${{ github.event.pull_request.html_url }}"
```

`check` exits with code `3` when any pin needs attention, which fails
the job; parse the JSON envelope instead of the exit code to treat
`Inapplicable` or `RequiresAdaptation` differently. Cache the snapshot
directory between runs: `snapshots generate` only scans new tags.

### Snapshot Staleness

`backhopper snapshots verify --all` re-parses every stored snapshot,
reports `verified: N, failed: M, stale_extractor: K`,
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
