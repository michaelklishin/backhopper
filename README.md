# backhopper

`backhopper` captures the public API of Erlang and Elixir projects at every
git tag and answers compatibility questions against those captures.

It is built to make backport automation safer. When a patch from a newer
release line is cherry-picked into an older one, the older line usually
pins older versions of its dependencies; a call to a function added in a
newer dep version then compiles fine on the source branch and breaks on
the target. `backhopper` turns that mismatch into a mechanical query.


## Intended Use Cases

`backhopper` fits projects that pin specific dependency versions per
release line and routinely cherry-pick patches across lines. The
compatibility command accepts a patch, a commit, a range, or a merge
commit, extracts every call site, record reference, and dispatch pattern,
then resolves each against the pinned snapshots.


## Project Maturity

Young project, breaking changes are likely.


## Binary Releases

Binary releases are available [on the Releases page](https://github.com/michaelklishin/backhopper/releases).


## Usage

### Getting Help

```shell
backhopper help
```

All command groups and individual commands support `--help`:

```shell
backhopper compatibility --help
backhopper compatibility commit --help

backhopper snapshots --help
backhopper api --help
```


### Configuration

`backhopper` reads a TOML config (`backhopper.toml` in the working
directory by default; override with `-c` or `BACKHOPPER_CONFIG_FILE_PATH`).

Each project gets a single entry; release lines are described as named
"series" that pin one tag per project. A small example:

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


### Capturing Snapshots

`snapshots discover` walks every tag of a project, parses the public
surface, and writes one canonical text file per tag:

```shell
backhopper snapshots discover --project lib_a
```

Re-running is incremental: only new tags are scanned.

```shell
backhopper snapshots update
```


### Looking Up the API at a Tag

For ad-hoc questions ("was this function exported at v1.8.0?"):

```shell
backhopper api lookup --project lib_a --tag v2.0.4 \
                      --mfa some_module:some_function/2

backhopper api modules --project lib_a --tag v2.0.4

backhopper api diff --project lib_a --from v2.0.3 --to v2.0.4
```


### Checking a Patch Against a Series

The core operation. Given a commit on a newer branch, verify it lands
cleanly against a series that pins older dependency versions:

```shell
backhopper compatibility commit --series stable-3.x \
                                --repo /path/to/your_repo.git \
                                1a2b3c4d
```

Short SHAs, tags, or any rev spec `git rev-parse` understands are fine.

For a commit range, or for a merge commit (which expands to its first
parent against the merge SHA):

```shell
backhopper compatibility range --series stable-3.x \
                               --repo /path/to/your_repo.git \
                               --range v3.0.0..HEAD

backhopper compatibility range --series stable-3.x \
                               --repo /path/to/your_repo.git \
                               --merge-commit 9f8e7d6c
```

A raw unified diff also works, piped or from a file:

```shell
git format-patch -1 --stdout HEAD | \
  backhopper compatibility patch --series stable-3.x

backhopper compatibility patch --series stable-3.x /path/to/the.patch
```

A clean run is terse on purpose: one row per pin, the verdict, and the
count of tracked symbols that were checked.

```
compatible: 2, requires_adaptation: 0, incompatible: 0

┌──────────────────┬────────────┬────────┬──────────────────────────────┐
│ pin              │ verdict    │ reason │ detail                       │
├──────────────────┼────────────┼────────┼──────────────────────────────┤
│ lib_a@v2.0.4     │ Compatible │ -      │ 3 tracked symbols referenced │
│ lib_b@v1.8.0     │ Compatible │ -      │ 0 tracked symbols referenced │
└──────────────────┴────────────┴────────┴──────────────────────────────┘
```

A `0 tracked symbols referenced` pin means the patch never touched that
project's surface, so the verdict is unconditional. A non-zero count is
the trust signal: `backhopper` actually checked that many call sites.


### Seeing What Was Skipped

The verdict only covers tracked projects. Anything else (OTP stdlib,
third-party libraries you didn't configure, dynamic dispatch through
variables) is kept out so it doesn't drown the signal. Add
`--show-untracked-calls` to see what was skipped:

```shell
backhopper compatibility commit --series stable-3.x \
                                --repo /path/to/your_repo.git \
                                --show-untracked-calls 1a2b3c4d
```

The footer groups what it found into three categories, each strictly
informational:

* **Untracked module calls**: calls to modules owned by no tracked
  project. The annotation suggests a candidate project name (a module
  named `something_helper` is reported as "project: something?") so it's
  easy to tell whether you forgot to track something versus whether the
  call is genuinely outside the tool's scope
* **Untracked records**: `#name{...}` references for records that no
  tracked snapshot declares. Same shape as untracked calls but for the
  record namespace
* **Unanalyzed dynamic dispatch**: `apply/3`, the `spawn` family, and
  `Mod:fun(...)` patterns where the module or function name is a variable.
  These can't be resolved statically; `backhopper` counts them so they're
  visible rather than silently dropped

OTP stdlib calls (`lists`, `gen_server`, `application`, ...) are
suppressed by default even with `--show-untracked-calls` on, because they
rarely change the conclusion. Add `--show-otp-calls` to include them.


### JSON Output

Every command accepts `--formatter json`:

```shell
backhopper --formatter json compatibility commit \
    --series stable-3.x \
    --repo /path/to/your_repo.git \
    1a2b3c4d
```

Diagnostics live under `data.diagnostics`, strictly separate from
`data.results` so a JSON consumer cannot mistake them for actionable
verdicts.


## Subprojects

 * `crates/backhopper-core` is the library: model types, snapshot I/O,
   store, `gix`-based git access, and the compatibility analyzer
 * `crates/backhopper-erlang` is the Erlang source surface extractor
 * `crates/backhopper-elixir` is the Elixir source surface extractor
 * `crates/backhopper-cli` is the `backhopper` command line tool


## License

This project is double licensed under the MIT License and the Apache License, Version 2.0.

See `LICENSE-APACHE` and `LICENSE-MIT` for details.

SPDX-License-Identifier: Apache-2.0 OR MIT


## Copyright

(c) 2026 Michael S. Klishin and Contributors.
