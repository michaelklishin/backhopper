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

`backhopper` turns the tables, allowing the essential part of such API
compatibility verification to happen before backporting. It records the
public API of each dependency at every git tag, then checks any patch,
commit, range, or merge commit against the dependency versions a target
branch uses.


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

Three commands take you from an empty directory to something you can
actually check against:

```shell
backhopper init                  # write a starter backhopper.toml
backhopper doctor                # see what's configured and what's missing
backhopper snapshots generate    # fill in the missing snapshots
```

`init` drops a `backhopper.toml` next to you with an absolute
`snapshot_dir` (so it keeps working when you `cd` away later) and a
commented-out `[[project]]` example to crib from.

`doctor` is the "is everything ready?" view: it lists each series, the
pin it expects, whether a snapshot is on disk, and, if anything is
missing, the exact command you'd run to fix it. Pass `--check-remote`
and it also tells you how many new tags each project has upstream past
the latest one you've captured.

Once you have a project or two in `backhopper.toml`, the day-to-day
loop is mostly `backhopper check ...` (covered below) — and when a
patch references a module you didn't know to track, `--suggest-projects`
will print a copy-pasteable `[[project]]` stub for it.

If you happen to be working on RabbitMQ, `init` can also seed itself
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
* `--formatter json|text` (env `BACKHOPPER_FORMATTER`): default `json`
* `--quiet` (`-q`): drop everything except errors on stderr
* `--verbose` (`-v`): bump log verbosity. `-v` is info, `-vv` is debug, `-vvv` is trace. `RUST_LOG` wins if set
* `--non-interactive` (env `BACKHOPPER_NON_INTERACTIVE_MODE`): turn off progress spinners and anything that would otherwise prompt; the CI-friendly default
* `--table-style modern|ascii|markdown|psql`: pick the look of text-mode tables

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


### Tracking Erlang/OTP

`backhopper` ships first-class support for Erlang/OTP, which is a
multi-app monorepo of ~35 applications under `lib/`, plus `erts/`. Set
`layout = "erlang_otp"` and a sensible default policy is applied:

```toml
[[project]]
name    = "otp"
git_url = "/path/to/otp.git"
layout  = "erlang_otp"
```

The defaults that snap into place (overridable field by field):

* `app_roots`: `["lib/*", "erts/preloaded"]`
* `exclude_apps`: `odbc`, `snmp`, `ssh`, `tftp`, `ftp`, `wx`, `megaco`,
  `edoc`, `jinterface`, `diameter` (the RabbitMQ `erlang-rpm` exclusion
  set)
* `excluded_subdirs`: `doc`, `example`, `examples`, `test` (matches the
  zero-dependency Erlang RPM from Team RabbitMQ plus CT suites, and a
  few library-specific idiosyncracies)
* `tag_pattern`: `OTP-*`
* `min_tag`: `OTP-26.0` (filters out older tags; `oldest_tag` is accepted as an ailas)
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
```


### Diffing the API Between Two Tags

`snapshots diff` emits a structured delta across modules, exports,
callbacks, types, headers, and records in a text format.

The format is very minimalistic: just `added` and `removed` prefixes,
and each line is module-qualified:

```shell
backhopper snapshots diff --project ra --from v2.15.2 --to v3.1.2
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

An arity change will result in one removal and one addition to the same
function (or type).

To diff two whole release series at once, one section per project:

```shell
backhopper snapshots diff --from-series stable-3.x --to-series stable-4.x
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

For a commit range, or for a merge commit (which expands to the diff
between the merge's first parent and the merge itself):

```shell
backhopper check range --series stable-3.x \
                       --repo-dir-path /path/to/your_repo.git \
                       --range v3.0.0..HEAD

backhopper check range --series stable-3.x \
                       --repo-dir-path /path/to/your_repo.git \
                       --merge-commit 9f8e7d6c
```

A raw unified diff also works, either piped in or read from a file:

```shell
git format-patch -1 --stdout HEAD | \
  backhopper check patch --series stable-3.x

backhopper check patch --series stable-3.x /path/to/the.patch
```

For many commits at once, `check batch` evaluates each commit against one
or more series in a single invocation:

```shell
backhopper check batch \
    --series stable-3.x,stable-4.x \
    --repo-dir-path /path/to/your_repo.git \
    --commits-file-path candidates.txt
```

For one commit against several series, `check multi` is the focused form:

```shell
backhopper check multi \
    --series stable-3.x,stable-4.x \
    1a2b3c4d
```

To skip cloning, `check pr` resolves a GitHub PR URL via the `gh` CLI:

```shell
backhopper check pr --series stable-3.x \
    https://github.com/owner/repo/pull/123
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
project's API surface, so the verdict is trivially safe. A non-zero
count is the trust signal: `backhopper` actually checked that many
call sites against the pinned tag.


### Verdicts and Exit Codes

Every per-pin verdict is one of four values; the process exit code is
the worst across the series:

| Verdict | Exit | Meaning |
|---|---:|---|
| `Compatible` | `0` | The patch's tracked references all resolve at the pinned tag |
| `RequiresAdaptation` | `3` | Non-blocking findings: deprecated usage, context drift, unsupported file types |
| `Incompatible` | `2` | Blocking findings: missing symbol, arity change, hidden module, missing prereq |
| `Inapplicable` | `4` | The diff touched no analyzable Erlang surface at this pin (docs-only, schema-only, test-only) |

`Inapplicable` is distinct from `Compatible (0 tracked symbols referenced)`:
the latter says "we checked and found nothing of yours referenced", the
former says "there was nothing to check". Agents and scripts can branch
on the exit code to route to a different verification step.

`--terse` produces one JSON line for shell consumption:

```shell
backhopper check commit --series stable-3.x --terse 1a2b3c4d
# {"summary":"compatible","pins":2,"scope":"source","exit":0}
```


### Explaining a Non-Zero Count

The verdict table reports metrics suc has `tracked symbols referenced: N` but not
which call sites contributed. Adding `--explain` will report the details per pinned
dependency:

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


### Seeing What Was Skipped

The verdict only covers tracked projects. Calls to anything else — the
OTP standard library, third-party libraries you did not configure, or
dynamic dispatch through variables — are deliberately left out so they
do not drown the signal. Add `--show-untracked-calls` to see what was
skipped:

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


## Snapshot Staleness in CI

`backhopper snapshots verify --all` is the canonical CI gate. It re-parses
every stored snapshot, reports `verified: N, failed: M, stale_extractor: K`,
and exits non-zero on any parse failure. A non-zero `stale_extractor` count
signals snapshots written by an older extractor than the running binary; rerun
`backhopper snapshots rebuild --project <X> --tag <Y>` for each entry.


## Subprojects

 * `crates/backhopper-core` is the library: model types, snapshot I/O,
   store, `gix`-based git access, and the compatibility analyzer
 * `crates/backhopper-erlang` is the Erlang source surface extractor
 * `crates/backhopper-elixir` is the Elixir source surface extractor
 * `crates/backhopper-cuttlefish` parses Cuttlefish `.schema` files and
   feeds the embedded Erlang fun bodies through the Erlang extractor
 * `crates/backhopper-xref-graph` provides the call-graph primitives
   (vertices, relations, set algebra, transitive closure)
 * `crates/backhopper-xref-reader` turns Erlang source into call-graph
   input
 * `crates/backhopper-xref` is the cross-reference façade and the
   test-suite selection adapter
 * `crates/backhopper-cli` is the `backhopper` command line tool


## License

This project is double licensed under the MIT License and the Apache License, Version 2.0.

See `LICENSE-APACHE` and `LICENSE-MIT` for details.

SPDX-License-Identifier: Apache-2.0 OR MIT


## Copyright

(c) 2026 Michael S. Klishin and Contributors.
